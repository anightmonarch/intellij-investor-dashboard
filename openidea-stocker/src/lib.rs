//! Stocker for OpenIDEA — a 1:1 functional port of the JetBrains Stocker plugin.
//!
//! Architecture: the UI is a WebView (`web/index.html`) that owns all rendering;
//! this Rust side owns state, persistence, network and parsing. The two talk over
//! the `openidea.postMessage` / `host::post_view_message` JSON bridge.
//!
//! ## Why one market per call
//!
//! OpenIDEA gives a plugin call a hard 5 s wall clock (`Limits::timeout`), and
//! reaps the instance on a timeout; three consecutive failures stop the plugin
//! for the session. The original plugin fetched four markets plus four index
//! sets from a background `ScheduledExecutorService`, which is fine on the JVM
//! but cannot be done inside one host call here. So the page drives the refresh
//! loop: it asks for one market at a time and this side answers with that
//! market's quotes. Each call makes exactly one HTTP request, mirroring
//! `StockerApp.createConsolidatedUpdateThread` one market at a time.

mod fetcher;
mod model;
mod parser;
mod pinyin;
mod suggest;

use std::collections::BTreeMap;

use model::{Market, Quote, State};
use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    host, register_plugin, Context, Error, Event, EventKind, Invocation, LogLevel, Plugin,
};
use serde::{Deserialize, Serialize};

const VIEW: &str = "stocker";
const STATE_KEY: &str = "stocker.state.v1";

#[derive(Default)]
pub struct Main {
    state: State,
    /// Cached quotes per market (`cn`, `hk`, `us`, `crypto`) and indices (`cn_index`, …).
    quotes: BTreeMap<String, Vec<Quote>>,
}

impl Main {
    // ---- persistence -------------------------------------------------------

    fn load_state(&mut self) {
        match host::storage::get(STATE_KEY) {
            Ok(Some(json)) => match serde_json::from_str::<State>(&json) {
                Ok(s) => self.state = s,
                Err(e) => host::log(LogLevel::Warn, &format!("Stocker: bad saved state: {}", e)),
            },
            Ok(None) => {}
            Err(e) => host::log(LogLevel::Warn, &format!("Stocker: storage read failed: {}", e)),
        }
        self.apply_host_settings();
    }

    /// Merge values from OpenIDEA's generated settings page (schema-backed).
    /// The user's choice there overrides the persisted defaults.
    fn apply_host_settings(&mut self) {
        if let Some(p) = host::setting_string("quoteProvider") {
            self.state.quote_provider = match p.as_str() {
                "tencent" => model::Provider::Tencent,
                _ => model::Provider::Sina,
            };
        }
        if let Some(cp) = host::setting_string("colorPattern") {
            self.state.color_pattern = match cp.as_str() {
                "gurd" => model::ColorPattern::GreenUpRedDown,
                "none" => model::ColorPattern::None,
                _ => model::ColorPattern::RedUpGreenDown,
            };
        }
        if let Some(py) = host::setting_bool("displayNameWithPinyin") {
            self.state.display_name_with_pinyin = py;
        }
        // Not in the native schema (the original keeps it out of its settings
        // page too): read it only if something sets it, never clamp it to the
        // scheduler's 60 s floor — the page drives the refresh loop.
        if let Some(ri) = host::setting_number("refreshInterval") {
            self.state.refresh_interval = ri.max(1.0) as u64;
        }
    }

    fn save_state(&self) {
        if let Ok(json) = serde_json::to_string(&self.state) {
            if let Err(e) = host::storage::set(STATE_KEY, &json) {
                host::log(LogLevel::Warn, &format!("Stocker: storage write failed: {}", e));
            }
        }
    }

    fn provider_for(&self, market: Market) -> model::Provider {
        if market == Market::Crypto {
            self.state.crypto_quote_provider
        } else {
            self.state.quote_provider
        }
    }

    // ---- quote refresh -----------------------------------------------------

    /// Fetch one market (favorites + its index codes in a single request) and
    /// push the result to the page. One HTTP request: well inside the 5 s call
    /// budget. Mirrors one iteration of `fetchQuotesIfActive` in the original.
    fn refresh_market(&mut self, market: Market) {
        let provider = self.provider_for(market);
        let indices_codes = index_codes_vec(market);
        let mut codes = self.state.list(market).to_vec();
        codes.extend(indices_codes.iter().cloned());

        // Nothing to ask for: answer with empty lists so the page clears the tab.
        if codes.is_empty() {
            self.quotes.insert(market.key().to_string(), Vec::new());
            self.quotes.insert(format!("{}_index", market.key()), Vec::new());
            self.reply(&OutMsg::Quotes {
                market: market.key().to_string(),
                items: Vec::new(),
                indices: Vec::new(),
            });
            return;
        }

        let mut quotes = fetcher::quotes(provider, market, &codes);
        self.decorate(&mut quotes);

        // Index rows are the ones we asked for by their index code (the provider
        // answers in its own case/prefix, so compare case-insensitively).
        let is_index = |code: &str| indices_codes.iter().any(|i| i.eq_ignore_ascii_case(code));
        let indices: Vec<Quote> = quotes.iter().filter(|q| is_index(&q.code)).cloned().collect();
        let items: Vec<Quote> = quotes.into_iter().filter(|q| !is_index(&q.code)).collect();

        self.quotes.insert(market.key().to_string(), items.clone());
        self.quotes.insert(format!("{}_index", market.key()), indices.clone());
        self.reply(&OutMsg::Quotes { market: market.key().to_string(), items, indices });
    }

    /// Attach derived fields (cost, holdings, display name) to quotes.
    fn decorate(&self, quotes: &mut [Quote]) {
        for q in quotes.iter_mut() {
            q.display_name = self.state.display_name(&q.code, &q.name);
            q.cost = self.state.cost_of(&q.code);
            q.holdings = self.state.holdings_of(&q.code);
        }
    }

    /// Ask the page to run a refresh round (it then asks for one market at a
    /// time). Used after every mutation, so the table reflects the watchlist
    /// without this call having to hit the network.
    fn request_tick(&self) {
        self.reply(&OutMsg::Tick);
    }

    fn clear_quotes(&mut self) {
        self.quotes.clear();
        self.reply(&OutMsg::Cleared);
    }

    fn push_state(&self) {
        self.reply(&OutMsg::State { state: &self.state });
    }

    // ---- message bridge ----------------------------------------------------

    fn reply(&self, msg: &OutMsg) {
        if let Ok(json) = serde_json::to_string(msg) {
            if let Err(e) = host::post_view_message(VIEW, &json) {
                host::log(LogLevel::Warn, &format!("Stocker: post_view_message failed: {}", e));
            }
        }
    }

    fn handle_message(&mut self, raw: &str) {
        let msg: InMsg = match serde_json::from_str(raw) {
            Ok(m) => m,
            Err(e) => {
                host::log(LogLevel::Warn, &format!("Stocker: bad message {:?}: {}", raw, e));
                return;
            }
        };
        match msg {
            // The page just loaded: hand it the state and whatever is cached.
            InMsg::Ready => {
                self.push_state();
                for market in Market::all() {
                    let items = self.quotes.get(market.key()).cloned().unwrap_or_default();
                    let indices =
                        self.quotes.get(&format!("{}_index", market.key())).cloned().unwrap_or_default();
                    self.reply(&OutMsg::Quotes { market: market.key().to_string(), items, indices });
                }
                self.request_tick();
            }
            InMsg::Refresh { market } => match market {
                Some(m) => self.refresh_market(parse_market(&m)),
                None => self.request_tick(),
            },
            InMsg::Stop => self.clear_quotes(),
            InMsg::Add { market, code, name } => {
                let m = parse_market(&market);
                if self.state.contains(&code) {
                    return;
                }
                let provider = self.provider_for(m);
                // One HTTP request, exactly like `StockerActionUtil.addStock`.
                if fetcher::validate_code(m, provider, &code) {
                    self.state.list_mut(m).push(code.clone());
                    self.save_state();
                    self.reply(&OutMsg::Added { code, name });
                    self.push_state();
                    self.request_tick();
                } else {
                    self.reply(&OutMsg::Error { message: format!("{} 无法获取行情，未添加", name) });
                }
            }
            InMsg::Remove { code } => {
                self.state.remove(&code);
                self.save_state();
                self.reply(&OutMsg::Removed { code });
                self.push_state();
                self.request_tick();
            }
            InMsg::Reorder { market, codes } => {
                // Mirrors StockerManagementDialog OK: the dialog's list becomes
                // the whole watchlist for that market, preserving order.
                let m = parse_market(&market);
                *self.state.list_mut(m) = codes;
                self.save_state();
                self.push_state();
                self.request_tick();
            }
            InMsg::SetMeta { code, custom_name, cost, holdings } => {
                if let Some(n) = custom_name {
                    if n.trim().is_empty() {
                        self.state.custom_names.remove(&code);
                    } else {
                        self.state.custom_names.insert(code.clone(), n);
                    }
                }
                match cost {
                    Some(v) => {
                        self.state.cost_prices.insert(code.clone(), v);
                    }
                    None => {
                        self.state.cost_prices.remove(&code);
                    }
                }
                match holdings {
                    Some(v) => {
                        self.state.holdings.insert(code.clone(), v);
                    }
                    None => {
                        self.state.holdings.remove(&code);
                    }
                }
                self.save_state();
                self.push_state();
                self.request_tick();
            }
            InMsg::Suggest { key, mode } => {
                let markets: Vec<Market> = if mode == "crypto" {
                    vec![Market::Crypto]
                } else {
                    vec![Market::AShare, Market::Hk, Market::Us]
                };
                let provider = if mode == "crypto" {
                    self.state.crypto_quote_provider
                } else {
                    self.state.quote_provider
                };
                let url = format!("{}{}", provider.suggest_host(), key);
                let req = openidea_plugin::HttpRequest::get(&url);
                match host::fetch(&req) {
                    Ok(resp) => {
                        let result = suggest::filter(suggest::parse(provider, &resp.body), &markets);
                        self.reply(&OutMsg::Suggestions { items: result });
                    }
                    Err(e) => {
                        host::log(LogLevel::Warn, &format!("Stocker suggest failed: {}", e));
                        self.reply(&OutMsg::Error {
                            message: format!(
                                "搜索失败：{}。请在 设置 › 插件 › Stocker 中允许网络权限。",
                                e
                            ),
                        });
                        self.reply(&OutMsg::Suggestions { items: Vec::new() });
                    }
                }
            }
            InMsg::UpdateSettings { state } => {
                // Merge a partial patch so watchlists/metadata are never wiped.
                if let Some(v) = state.language_override {
                    self.state.language_override = v;
                }
                if let Some(v) = state.quote_provider {
                    self.state.quote_provider = v;
                }
                if let Some(v) = state.crypto_quote_provider {
                    self.state.crypto_quote_provider = v;
                }
                if let Some(v) = state.display_name_with_pinyin {
                    self.state.display_name_with_pinyin = v;
                }
                if let Some(v) = state.color_pattern {
                    self.state.color_pattern = v;
                }
                if let Some(v) = state.visible_columns {
                    if !v.is_empty() {
                        self.state.visible_columns = v;
                    }
                }
                if let Some(v) = state.refresh_interval {
                    self.state.refresh_interval = v.max(1);
                }
                self.save_state();
                self.push_state();
                self.request_tick();
            }
            InMsg::Clear => {
                for market in Market::all() {
                    self.state.list_mut(market).clear();
                }
                self.save_state();
                self.push_state();
                self.clear_quotes();
            }
        }
    }
}

fn index_codes_vec(market: Market) -> Vec<String> {
    model::index_codes(market).iter().map(|s| s.to_string()).collect()
}

fn parse_market(s: &str) -> Market {
    match s {
        "hk" => Market::Hk,
        "us" => Market::Us,
        "crypto" => Market::Crypto,
        _ => Market::AShare,
    }
}

// ---- JSON protocol ---------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum InMsg {
    Ready,
    Refresh {
        #[serde(default)]
        market: Option<String>,
    },
    Stop,
    Clear,
    Add {
        market: String,
        code: String,
        name: String,
    },
    Remove {
        code: String,
    },
    Reorder {
        market: String,
        codes: Vec<String>,
    },
    SetMeta {
        code: String,
        // `rename_all` on an enum renames the variants, not their fields.
        #[serde(default, rename = "customName")]
        custom_name: Option<String>,
        #[serde(default)]
        cost: Option<f64>,
        #[serde(default)]
        holdings: Option<i64>,
    },
    Suggest {
        key: String,
        #[serde(default)]
        mode: String,
    },
    UpdateSettings {
        state: SettingsPatch,
    },
}

/// Partial settings update from the WebView settings dialog.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    #[serde(default)]
    language_override: Option<String>,
    #[serde(default)]
    quote_provider: Option<model::Provider>,
    #[serde(default)]
    crypto_quote_provider: Option<model::Provider>,
    #[serde(default)]
    display_name_with_pinyin: Option<bool>,
    #[serde(default)]
    color_pattern: Option<model::ColorPattern>,
    #[serde(default)]
    visible_columns: Option<Vec<String>>,
    #[serde(default)]
    refresh_interval: Option<u64>,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum OutMsg<'a> {
    State {
        state: &'a State,
    },
    Quotes {
        market: String,
        items: Vec<Quote>,
        indices: Vec<Quote>,
    },
    Added {
        code: String,
        name: String,
    },
    Removed {
        code: String,
    },
    Error {
        message: String,
    },
    Suggestions {
        items: Vec<model::Suggestion>,
    },
    /// Start a refresh round (one market per call).
    Tick,
    /// The table is empty: drop every row.
    Cleared,
}

impl Plugin for Main {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        self.load_state();
        let _ = host::subscribe(EventKind::Scheduled);
        let _ = host::subscribe(EventKind::SettingsChanged);
        Ok(())
    }

    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            // Commands never touch the network: the page owns the refresh loop,
            // so a command only has to wake it up.
            "refresh" => {
                self.request_tick();
                Ok(Some("Quotes refreshed".into()))
            }
            "stop" => {
                self.clear_quotes();
                Ok(Some("Refresh stopped".into()))
            }
            "clear" => {
                for market in Market::all() {
                    self.state.list_mut(market).clear();
                }
                self.save_state();
                self.push_state();
                self.clear_quotes();
                Ok(Some("Favorites cleared".into()))
            }
            "settings" => {
                host::reveal_view(VIEW)?;
                let _ = host::post_view_message(VIEW, r#"{"op":"openSettings"}"#);
                Ok(None)
            }
            "add" | "manage" => {
                host::reveal_view(VIEW)?;
                let _ = host::post_view_message(
                    VIEW,
                    if id == "add" { r#"{"op":"openSearch"}"# } else { r#"{"op":"openManage"}"# },
                );
                Ok(None)
            }
            _ => Err(format!("unknown command {id}").into()),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for e in events {
            match e {
                // A scheduled tick only nudges the page; the fetch happens in the
                // page's per-market round, inside the 5 s budget.
                Event::Scheduled => self.request_tick(),
                Event::Other { kind, .. } if kind == "settings-changed" => {
                    self.apply_host_settings();
                    self.save_state();
                    self.push_state();
                    self.request_tick();
                }
                _ => {}
            }
        }
    }
}

impl Views for Main {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        // The WebView renders itself; return an empty message body.
        Ok(View::message(""))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if let ViewEvent::Message(raw) = event {
            self.handle_message(&raw);
        }
        Ok(())
    }
}

register_plugin!(Main, views);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ColorPattern, Provider};
    use openidea_plugin::mock::MockHost;

    #[test]
    fn loads_default_state() {
        let host = MockHost::install();
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        assert_eq!(p.state.quote_provider, Provider::Sina);
        assert_eq!(p.state.color_pattern, ColorPattern::RedUpGreenDown);
    }

    #[test]
    fn add_and_remove_favorites() {
        let host = MockHost::install().with_capabilities(&["storage", "net:hq.sinajs.cn"]);
        host.on_fetch(|_| {
            Ok(openidea_plugin::HttpResponse {
                status: 200,
                headers: vec![],
                body: b"var hq_str_sh600000=\"x,1,2,3\";".to_vec(),
            })
        });
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        p.handle_message(r#"{"op":"add","market":"cn","code":"sh600000","name":"test"}"#);
        assert!(p.state.contains("sh600000"));
        p.handle_message(r#"{"op":"remove","code":"sh600000"}"#);
        assert!(!p.state.contains("sh600000"));
    }

    /// The page's whole protocol, as the WebView sees it: `ready` answers with
    /// the state and one `quotes` per market, and every mutation ends with a
    /// `tick` so the page starts a round instead of this call hitting the
    /// network (which would blow the 5 s budget).
    #[test]
    fn ready_and_mutations_reply_in_the_page_protocol() {
        let host = MockHost::install().with_capabilities(&["storage", "net:hq.sinajs.cn"]);
        host.on_fetch(|_| {
            Ok(openidea_plugin::HttpResponse {
                status: 200,
                headers: vec![],
                body: b"var hq_str_sh600000=\"x,1,2,3\";".to_vec(),
            })
        });
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));

        p.handle_message(r#"{"op":"ready"}"#);
        let ops = |host: &MockHost| -> Vec<String> {
            host.view_messages()
                .iter()
                .map(|(_, m)| {
                    serde_json::from_str::<serde_json::Value>(m)
                        .ok()
                        .and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_owned))
                        .unwrap_or_default()
                })
                .collect()
        };
        let seen = ops(&host);
        assert_eq!(seen.first().map(String::as_str), Some("state"), "state first: {seen:?}");
        assert_eq!(seen.iter().filter(|o| *o == "quotes").count(), 4, "one per market: {seen:?}");
        assert_eq!(seen.last().map(String::as_str), Some("tick"), "then a round: {seen:?}");

        // A refresh names the market it answered for, so the page can merge it.
        host.view_messages().clear();
        p.handle_message(r#"{"op":"refresh","market":"cn"}"#);
        let raw = host.view_messages().pop().expect("a reply").1;
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["op"], "quotes");
        assert_eq!(v["market"], "cn");
        assert!(v["items"].is_array() && v["indices"].is_array());

        // Adding must not fetch the quotes itself: it validates once, then ticks.
        host.view_messages().clear();
        p.handle_message(r#"{"op":"add","market":"cn","code":"sh600000","name":"浦发银行"}"#);
        let seen = ops(&host);
        assert!(seen.contains(&"state".to_string()), "state after add: {seen:?}");
        assert_eq!(seen.last().map(String::as_str), Some("tick"), "tick after add: {seen:?}");
    }

    /// `setMeta` sends `customName`; an enum's `rename_all` does not touch its
    /// fields, so this guards the explicit rename.
    #[test]
    fn set_meta_accepts_camel_case_fields() {
        let host = MockHost::install().with_capabilities(&["storage"]);
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        p.state.list_mut(Market::AShare).push("SH600000".into());
        p.handle_message(
            r#"{"op":"setMeta","code":"SH600000","customName":"我的股票","cost":9.5,"holdings":100}"#,
        );
        assert_eq!(p.state.display_name("SH600000", "浦发银行"), "我的股票");
        assert_eq!(p.state.cost_of("sh600000"), Some(9.5));
        assert_eq!(p.state.holdings_of("sh600000"), Some(100));
    }

    /// The index codes are matched case-insensitively against what the provider
    /// answers (`sh000001`), so the index bar is never empty.
    #[test]
    fn index_rows_are_split_out_of_the_quote_list() {
        let host = MockHost::install().with_capabilities(&["storage", "net:hq.sinajs.cn"]);
        host.on_fetch(|_| {
            Ok(openidea_plugin::HttpResponse {
                status: 200,
                headers: vec![],
                body: concat!(
                    "var hq_str_sh600000=\"浦发银行,10.00,10.50,10.80,11.00,9.90,,,,,,,,,,,,,,,,,,,,,,,,,,,,,2026-11-26,13:20:00\";\n",
                    "var hq_str_sh000001=\"上证指数,3400.00,3410.00,3481.04,3490.00,3390.00,,,,,,,,,,,,,,,,,,,,,,,,,,,,,2026-11-26,13:20:00\";"
                )
                .as_bytes()
                .to_vec(),
            })
        });
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        p.state.list_mut(Market::AShare).push("sh600000".into());
        p.refresh_market(Market::AShare);
        assert_eq!(p.quotes["cn"].len(), 1);
        assert_eq!(p.quotes["cn"][0].code, "SH600000");
        assert_eq!(p.quotes["cn_index"].len(), 1);
        assert_eq!(p.quotes["cn_index"][0].code, "SH000001");
    }
}

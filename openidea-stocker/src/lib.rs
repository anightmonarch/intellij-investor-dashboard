//! Stocker for OpenIDEA — a 1:1 functional port of the JetBrains Stocker plugin.
//!
//! Architecture: the UI is a WebView (`web/index.html`) that owns all rendering;
//! this Rust side owns state, persistence, network and parsing. The two talk over
//! the `openidea.postMessage` / `host::post_view_message` JSON bridge.

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
    /// Cached quotes per market (including the merged "all" under key "all").
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
        if let Some(ri) = host::setting_number("refreshInterval") {
            self.state.refresh_interval = ri.max(60.0) as u64;
        }
    }

    fn save_state(&self) {
        if let Ok(json) = serde_json::to_string(&self.state) {
            if let Err(e) = host::storage::set(STATE_KEY, &json) {
                host::log(LogLevel::Warn, &format!("Stocker: storage write failed: {}", e));
            }
        }
    }

    // ---- quote refresh -----------------------------------------------------

    /// Fetch all four markets plus indices once, merge cost/holdings/display name,
    /// and store the result for the WebView to read.
    fn refresh(&mut self) {
        let mut all: Vec<Quote> = Vec::new();
        for market in Market::all() {
            let provider = if market == Market::Crypto {
                self.state.crypto_quote_provider
            } else {
                self.state.quote_provider
            };
            let mut codes = self.state.list(market).to_vec();
            codes.extend(index_codes_vec(market));
            let mut quotes = fetcher::quotes(provider, market, &codes);
            self.decorate(&mut quotes);
            // Favorites only (drop index rows) for the per-market tab.
            let favorites: Vec<Quote> = quotes
                .iter()
                .filter(|q| self.state.contains(&q.code))
                .cloned()
                .collect();
            all.extend(favorites.clone());
            self.quotes.insert(market.key().to_string(), favorites);
            // Indices stored separately for the index bar.
            let indices: Vec<Quote> = quotes
                .into_iter()
                .filter(|q| index_codes_vec(market).contains(&q.code))
                .collect();
            self.quotes.insert(format!("{}_index", market.key()), indices);
        }
        self.quotes.insert("all".into(), all);
        host::refresh_view(VIEW);
    }

    /// Attach derived fields (cost, holdings, display name) to quotes.
    fn decorate(&self, quotes: &mut [Quote]) {
        for q in quotes.iter_mut() {
            q.display_name = self.state.display_name(&q.code, &q.name);
            q.cost = self.state.cost_of(&q.code);
            q.holdings = self.state.holdings_of(&q.code);
        }
    }

    fn stop(&mut self) {
        self.quotes.clear();
        host::refresh_view(VIEW);
    }

    // ---- message bridge ----------------------------------------------------

    fn reply(&self, msg: &OutMsg) {
        if let Ok(json) = serde_json::to_string(msg) {
            let _ = host::post_view_message(VIEW, &json);
        }
    }

    fn handle_message(&mut self, raw: &str) {
        let msg: InMsg = match serde_json::from_str(raw) {
            Ok(m) => m,
            Err(e) => {
                host::log(LogLevel::Warn, &format!("Stocker: bad message: {}", e));
                return;
            }
        };
        match msg {
            InMsg::Ready => {
                self.reply(&OutMsg::State { state: &self.state });
                self.reply(&OutMsg::Quotes { quotes: &self.quotes });
            }
            InMsg::Refresh => self.refresh(),
            InMsg::Stop => self.stop(),
            InMsg::Add { market, code, name } => {
                let m = parse_market(&market);
                if !self.state.contains(&code) {
                    let provider = if m == Market::Crypto {
                        self.state.crypto_quote_provider
                    } else {
                        self.state.quote_provider
                    };
                    if fetcher::validate_code(m, provider, &code) {
                        self.state.list_mut(m).push(code.clone());
                        self.save_state();
                        self.reply(&OutMsg::Added { code, name });
                        self.refresh();
                    } else {
                        self.reply(&OutMsg::Error { message: format!("{} is not supported", name) });
                    }
                }
            }
            InMsg::Remove { code } => {
                self.state.remove(&code);
                self.save_state();
                self.reply(&OutMsg::Removed { code });
                self.refresh();
            }
            InMsg::Reorder { market, codes } => {
                // Mirrors StockerManagementDialog OK: the dialog's list becomes
                // the whole watchlist for that market, preserving order.
                let m = parse_market(&market);
                *self.state.list_mut(m) = codes;
                self.save_state();
                self.refresh();
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
                    Some(v) => { self.state.cost_prices.insert(code.clone(), v); }
                    None => { self.state.cost_prices.remove(&code); }
                }
                match holdings {
                    Some(v) => { self.state.holdings.insert(code.clone(), v as i64); }
                    None => { self.state.holdings.remove(&code); }
                }
                self.save_state();
                self.refresh();
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
                let result = match host::fetch(&req) {
                    Ok(resp) => suggest::filter(suggest::parse(provider, &resp.body), &markets),
                    Err(_) => Vec::new(),
                };
                self.reply(&OutMsg::Suggestions { items: result });
            }
            InMsg::UpdateSettings { state } => {
                // Merge a partial patch so watchlists/metadata are never wiped.
                if let Some(v) = state.language_override { self.state.language_override = v; }
                if let Some(v) = state.quote_provider { self.state.quote_provider = v; }
                if let Some(v) = state.crypto_quote_provider { self.state.crypto_quote_provider = v; }
                if let Some(v) = state.display_name_with_pinyin { self.state.display_name_with_pinyin = v; }
                if let Some(v) = state.color_pattern { self.state.color_pattern = v; }
                if let Some(v) = state.visible_columns { self.state.visible_columns = v; }
                if let Some(v) = state.refresh_interval { self.state.refresh_interval = v.max(60); }
                self.save_state();
                self.refresh();
            }
            InMsg::Clear => {
                for market in Market::all() {
                    self.state.list_mut(market).clear();
                }
                self.save_state();
                self.stop();
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
    Refresh,
    Stop,
    Clear,
    Add { market: String, code: String, name: String },
    Remove { code: String },
    Reorder { market: String, codes: Vec<String> },
    SetMeta { code: String, custom_name: Option<String>, cost: Option<f64>, holdings: Option<f64> },
    Suggest { key: String, mode: String },
    UpdateSettings { state: SettingsPatch },
}

/// Partial settings update from the WebView settings dialog.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    #[serde(default)] language_override: Option<String>,
    #[serde(default)] quote_provider: Option<model::Provider>,
    #[serde(default)] crypto_quote_provider: Option<model::Provider>,
    #[serde(default)] display_name_with_pinyin: Option<bool>,
    #[serde(default)] color_pattern: Option<model::ColorPattern>,
    #[serde(default)] visible_columns: Option<Vec<String>>,
    #[serde(default)] refresh_interval: Option<u64>,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum OutMsg<'a> {
    State { state: &'a State },
    Quotes { quotes: &'a BTreeMap<String, Vec<Quote>> },
    Added { code: String, name: String },
    Removed { code: String },
    Error { message: String },
    Suggestions { items: Vec<model::Suggestion> },
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
            "refresh" => { self.refresh(); Ok(Some("Quotes refreshed".into())) }
            "stop" => { self.stop(); Ok(Some("Refresh stopped".into())) }
            "clear" => {
                for market in Market::all() { self.state.list_mut(market).clear(); }
                self.save_state();
                self.stop();
                Ok(Some("Favorites cleared".into()))
            }
            "settings" => {
                host::reveal_view(VIEW)?;
                let _ = host::post_view_message(VIEW, r#"{"op":"openSettings"}"#);
                Ok(None)
            }
            "add" | "manage" => {
                host::reveal_view(VIEW)?;
                Ok(None)
            }
            _ => Err(format!("unknown command {id}").into()),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for e in events {
            match e {
                Event::Scheduled => self.refresh(),
                Event::Other { kind, .. } if kind == "settings-changed" => {
                    self.apply_host_settings();
                    self.save_state();
                    self.refresh();
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
    fn loads_default_state_and_replies() {
        let host = MockHost::install();
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        assert_eq!(p.state.refresh_interval, 5);
        assert_eq!(p.state.quote_provider, Provider::Sina);
        assert_eq!(p.state.color_pattern, ColorPattern::RedUpGreenDown);
    }

    #[test]
    fn add_and_remove_favorites() {
        let host = MockHost::install().with_capabilities(&["storage", "net:hq.sinajs.cn"]);
        host.on_fetch(|_| Ok(openidea_plugin::HttpResponse {
            status: 200,
            headers: vec![],
            body: b"var hq_str_sh600000=\"x,1,2,3\";".to_vec(),
        }));
        let mut p = Main::default();
        let _ = p.activate(&host.context("vermouth.stocker"));
        p.handle_message(r#"{"op":"add","market":"cn","code":"sh600000","name":"test"}"#);
        assert!(p.state.contains("sh600000"));
        p.handle_message(r#"{"op":"remove","code":"sh600000"}"#);
        assert!(!p.state.contains("sh600000"));
    }
}

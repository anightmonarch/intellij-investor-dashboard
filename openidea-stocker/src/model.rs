//! Data models ported from the original Stocker Kotlin plugin.

use serde::{Deserialize, Serialize};

/// A single quote snapshot. Mirrors `StockerQuote.kt`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub code: String,
    pub name: String,
    pub current: f64,
    pub opening: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub change: f64,
    pub percentage: f64,
    pub update_at: String,
    /// Cost price and holdings are merged from settings at render time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holdings: Option<i64>,
    /// Display name after custom-name / pinyin resolution (computed in Rust).
    pub display_name: String,
}

/// A search suggestion. Mirrors `StockerSuggestion.kt`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub code: String,
    pub name: String,
    pub market: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Market {
    #[serde(rename = "cn")]
    AShare,
    #[serde(rename = "hk")]
    Hk,
    #[serde(rename = "us")]
    Us,
    #[serde(rename = "crypto")]
    Crypto,
}

#[allow(dead_code)]
impl Market {
    pub fn key(self) -> &'static str {
        match self {
            Market::AShare => "cn",
            Market::Hk => "hk",
            Market::Us => "us",
            Market::Crypto => "crypto",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Market::AShare => "CN",
            Market::Hk => "HK",
            Market::Us => "US",
            Market::Crypto => "Crypto",
        }
    }

    pub fn all() -> [Market; 4] {
        [Market::AShare, Market::Hk, Market::Us, Market::Crypto]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Sina,
    Tencent,
}

impl Provider {
    pub fn host(self) -> &'static str {
        match self {
            Provider::Sina => "https://hq.sinajs.cn/list=",
            Provider::Tencent => "https://qt.gtimg.cn/q=",
        }
    }

    pub fn suggest_host(self) -> &'static str {
        match self {
            Provider::Sina => "https://suggest3.sinajs.cn/suggest/key=",
            Provider::Tencent => "https://smartbox.gtimg.cn/s3/?v=2&t=all&c=1&q=",
        }
    }

    /// Prefix prepended to a code when building the quote URL for a market.
    /// `None` means the provider does not support that market.
    pub fn prefix(self, market: Market) -> Option<&'static str> {
        match (self, market) {
            (Provider::Sina, Market::AShare) => Some(""),
            (Provider::Sina, Market::Hk) => Some("hk"),
            (Provider::Sina, Market::Us) => Some("gb_"),
            (Provider::Sina, Market::Crypto) => Some("btc_"),
            (Provider::Tencent, Market::AShare) => Some(""),
            (Provider::Tencent, Market::Hk) => Some("hk"),
            (Provider::Tencent, Market::Us) => Some("us"),
            (Provider::Tencent, Market::Crypto) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorPattern {
    #[serde(rename = "rugd")]
    RedUpGreenDown,
    #[serde(rename = "gurd")]
    GreenUpRedDown,
    #[serde(rename = "none")]
    None,
}

impl Default for ColorPattern {
    fn default() -> Self {
        ColorPattern::RedUpGreenDown
    }
}

/// All persistent state. Mirrors `StockerSettingState.kt`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub version: String,
    pub refresh_interval: u64,
    pub quote_provider: Provider,
    pub crypto_quote_provider: Provider,
    pub color_pattern: ColorPattern,
    pub display_name_with_pinyin: bool,
    pub language_override: String,
    pub visible_columns: Vec<String>,
    pub watchlists: std::collections::BTreeMap<String, Vec<String>>,
    pub custom_names: std::collections::BTreeMap<String, String>,
    pub cost_prices: std::collections::BTreeMap<String, f64>,
    pub holdings: std::collections::BTreeMap<String, i64>,
}

#[allow(dead_code)]
pub const ALL_COLUMNS: &[&str] = &[
    "symbol",
    "name",
    "current",
    "opening",
    "close",
    "low",
    "high",
    "change",
    "changePercent",
    "cost",
    "holdings",
    "netProfit",
    "dailyProfit",
];

impl Default for State {
    fn default() -> Self {
        let mut watchlists = std::collections::BTreeMap::new();
        for m in Market::all() {
            watchlists.insert(m.key().to_string(), Vec::new());
        }
        State {
            version: String::new(),
            refresh_interval: 5,
            quote_provider: Provider::Sina,
            crypto_quote_provider: Provider::Sina,
            color_pattern: ColorPattern::default(),
            display_name_with_pinyin: false,
            language_override: String::new(),
            visible_columns: vec!["name".into(), "current".into(), "changePercent".into()],
            watchlists,
            custom_names: std::collections::BTreeMap::new(),
            cost_prices: std::collections::BTreeMap::new(),
            holdings: std::collections::BTreeMap::new(),
        }
    }
}

impl State {
    pub fn list(&self, market: Market) -> &[String] {
        self.watchlists.get(market.key()).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn list_mut(&mut self, market: Market) -> &mut Vec<String> {
        self.watchlists.entry(market.key().to_string()).or_default()
    }

    /// Case-insensitive match: API codes come back uppercase (SH600887) while
    /// search suggestions may be lowercase (sh600887).
    pub fn contains(&self, code: &str) -> bool {
        let up = code.to_uppercase();
        self.watchlists.values().any(|v| v.iter().any(|c| c.to_uppercase() == up))
    }

    /// Return the stored code (in its original case) matching `code`, if any.
    #[allow(dead_code)]
    pub fn stored_code(&self, code: &str) -> Option<String> {
        let up = code.to_uppercase();
        self.watchlists.values().flatten().find(|c| c.to_uppercase() == up).cloned()
    }

    #[allow(dead_code)]
    pub fn market_of(&self, code: &str) -> Option<Market> {
        let up = code.to_uppercase();
        Market::all().into_iter().find(|m| self.list(*m).iter().any(|c| c.to_uppercase() == up))
    }

    pub fn remove(&mut self, code: &str) {
        let up = code.to_uppercase();
        for m in Market::all() {
            self.list_mut(m).retain(|c| c.to_uppercase() != up);
        }
        self.custom_names.retain(|k, _| k.to_uppercase() != up);
        self.cost_prices.retain(|k, _| k.to_uppercase() != up);
        self.holdings.retain(|k, _| k.to_uppercase() != up);
    }

    /// Display name priority: custom name > pinyin > original name.
    pub fn display_name(&self, code: &str, original: &str) -> String {
        let up = code.to_uppercase();
        if let Some((_, custom)) = self.custom_names.iter().find(|(k, _)| k.to_uppercase() == up) {
            if !custom.is_empty() {
                return custom.clone();
            }
        }
        if self.display_name_with_pinyin {
            return crate::pinyin::to_pinyin(original);
        }
        original.to_string()
    }

    pub fn cost_of(&self, code: &str) -> Option<f64> {
        let up = code.to_uppercase();
        self.cost_prices.iter().find(|(k, _)| k.to_uppercase() == up).map(|(_, v)| *v)
    }

    pub fn holdings_of(&self, code: &str) -> Option<i64> {
        let up = code.to_uppercase();
        self.holdings.iter().find(|(k, _)| k.to_uppercase() == up).map(|(_, v)| *v)
    }
}

/// Market index codes, ported from `StockerMarketIndex.kt`.
pub fn index_codes(market: Market) -> &'static [&'static str] {
    match market {
        Market::AShare => &["SH000001", "SZ399001", "SZ399006"],
        Market::Hk => &["HSI", "HSTECH"],
        Market::Us => &["DJI", "IXIC", "INX"],
        Market::Crypto => &["BTCBTCUSD"],
    }
}

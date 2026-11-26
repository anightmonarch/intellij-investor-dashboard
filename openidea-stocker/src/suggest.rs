//! Search-suggestion parsers ported from `StockerSuggestHttpUtil.kt`.

use crate::model::{Market, Provider, Suggestion};
use crate::parser;

/// Parse a suggestion response for the given provider.
pub fn parse(provider: Provider, body: &[u8]) -> Vec<Suggestion> {
    let text = parser::decode(body, provider);
    match provider {
        Provider::Sina => parse_sina(&text),
        Provider::Tencent => parse_tencent(&text),
    }
}

fn parse_sina(text: &str) -> Vec<Suggestion> {
    let mut result = Vec::new();
    let Some(start) = text.find('"') else { return result; };
    let Some(end) = text.rfind('"') else { return result; };
    if end <= start { return result; }
    let payload = &text[start + 1..end];
    for snippet in payload.split(';') {
        let cols: Vec<&str> = snippet.split(',').collect();
        if cols.len() < 5 { continue; }
        let code = cols[3];
        let name = cols[4];
        match cols[1] {
            "11" => {
                if !name.starts_with("S*ST") {
                    result.push(Suggestion { code: code.to_uppercase(), name: name.into(), market: "cn".into() });
                }
            }
            "22" => {
                let c = code.replace("of", "");
                let market_cn = if c.starts_with("15") || c.starts_with("16") || c.starts_with("18") {
                    Some(format!("SZ{}", c))
                } else if c.starts_with("50") || c.starts_with("51") {
                    Some(format!("SH{}", c))
                } else { None };
                if let Some(full) = market_cn {
                    result.push(Suggestion { code: full, name: name.into(), market: "cn".into() });
                }
            }
            "31" => result.push(Suggestion { code: code.to_uppercase(), name: name.into(), market: "hk".into() }),
            "41" => result.push(Suggestion { code: code.to_uppercase(), name: name.into(), market: "us".into() }),
            "71" => {
                let c = code.to_uppercase();
                if supported_crypto(&c) {
                    result.push(Suggestion { code: c, name: name.into(), market: "crypto".into() });
                }
            }
            "81" => result.push(Suggestion { code: code.to_uppercase(), name: name.into(), market: "cn".into() }),
            _ => {}
        }
    }
    result
}

/// Sina only quotes USD pairs with a BTC prefix: BTC{COIN}USD, 9-15 chars.
fn supported_crypto(code: &str) -> bool {
    if !code.starts_with("BTC") || !code.ends_with("USD") { return false; }
    if code.len() < 9 || code.len() > 15 { return false; }
    let coin = &code[3..code.len() - 3];
    coin.len() >= 3
}

fn parse_tencent(text: &str) -> Vec<Suggestion> {
    let mut result = Vec::new();
    if text.is_empty() { return result; }
    let payload = text.replace("v_hint=\"", "").replace('"', "");
    for snippet in payload.split('^') {
        let cols: Vec<&str> = snippet.split('~').collect();
        if cols.len() < 3 { continue; }
        let (kind, code, name) = (cols[0], cols[1], unescape_java(cols[2]));
        match kind {
            "sz" | "sh" => result.push(Suggestion {
                code: format!("{}{}", kind.to_uppercase(), code),
                name, market: "cn".into(),
            }),
            "hk" => result.push(Suggestion { code: code.into(), name, market: "hk".into() }),
            "us" => result.push(Suggestion {
                code: code.split('.').next().unwrap_or(code).to_uppercase(),
                name, market: "us".into(),
            }),
            _ => {}
        }
    }
    result
}

/// Minimal equivalent of Apache commons-text `unescapeJava` for `\uXXXX`.
fn unescape_java(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'u') {
            chars.next();
            let hex: String = chars.by_ref().take(4).collect();
            if let Ok(code) = u32::from_str_radix(&hex, 16) {
                if let Some(ch) = char::from_u32(code) {
                    out.push(ch);
                    continue;
                }
            }
            out.push('\\');
            out.push('u');
            out.push_str(&hex);
        } else {
            out.push(c);
        }
    }
    out
}

/// Filter suggestions to the markets allowed in the given search mode.
pub fn filter(suggestions: Vec<Suggestion>, markets: &[Market]) -> Vec<Suggestion> {
    let keys: Vec<&str> = markets.iter().map(|m| m.key()).collect();
    suggestions.into_iter().filter(|s| keys.contains(&s.market.as_str())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sina_suggestions_and_filters_crypto() {
        let body = r#"var suggestvalue="平安银行,11,600000,sh600000,平安银行;比特币,71,BTCBTCUSD,BTCBTCUSD,比特币;USDT,71,BTCBTCUSDT,BTCBTCUSDT,USDT";"#;
        let all = parse(Provider::Sina, body.as_bytes());
        assert!(all.iter().any(|s| s.code == "SH600000" && s.market == "cn"));
        assert!(all.iter().any(|s| s.code == "BTCBTCUSD" && s.market == "crypto"));
        assert!(!all.iter().any(|s| s.code.contains("USDT")), "USDT pairs must be rejected");
    }

    #[test]
    fn filters_by_market() {
        let s = vec![
            Suggestion { code: "A".into(), name: "a".into(), market: "cn".into() },
            Suggestion { code: "B".into(), name: "b".into(), market: "crypto".into() },
        ];
        let out = filter(s, &[Market::AShare]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "A");
    }

    #[test]
    fn unescapes_java_unicode() {
        assert_eq!(unescape_java("\\u5e73\\u5b89"), "平安");
    }
}

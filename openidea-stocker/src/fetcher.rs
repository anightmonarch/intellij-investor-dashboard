//! HTTP data access, ported from `StockerQuoteHttpUtil.kt` and `StockerHttpClientPool.kt`.
//!
//! Uses the host's `fetch` capability instead of Apache HttpClient. All requests
//! go through the sandboxed host; Sina requires a Referer header and GBK decoding.

use openidea_plugin::{host, Error, HttpRequest};
use crate::model::{Market, Provider, Quote};
use crate::parser;

/// Build the request URL for a batch of codes in one market.
/// Mirrors the prefix/case rules in `StockerQuoteHttpUtil.get`.
pub fn quote_url(provider: Provider, market: Market, codes: &[String]) -> Option<String> {
    let prefix = provider.prefix(market)?;
    let upper = matches!(
        (provider, market),
        (Provider::Sina, Market::Hk)
            | (Provider::Tencent, Market::Hk)
            | (Provider::Tencent, Market::Us)
    );
    let joined = codes
        .iter()
        .map(|c| {
            let code = if upper { c.to_uppercase() } else { c.to_lowercase() };
            format!("{}{}", prefix, code)
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(format!("{}{}", provider.host(), joined))
}

fn get(url: &str, provider: Provider) -> Result<String, Error> {
    let mut req = HttpRequest::get(url);
    if provider == Provider::Sina {
        req = req.header("Referer", "https://finance.sina.com.cn");
    }
    let resp = host::fetch(&req)?;
    if resp.status != 200 {
        return Err(format!("HTTP {}", resp.status).into());
    }
    Ok(parser::decode(&resp.body, provider))
}

/// Fetch and parse quotes for one market.
pub fn quotes(provider: Provider, market: Market, codes: &[String]) -> Vec<Quote> {
    if codes.is_empty() {
        return Vec::new();
    }
    let Some(url) = quote_url(provider, market, codes) else {
        return Vec::new();
    };
    match get(&url, provider) {
        Ok(text) => parser::parse(provider, market, &text),
        Err(e) => {
            host::log(openidea_plugin::LogLevel::Warn, &format!("Stocker fetch failed: {}", e));
            Vec::new()
        }
    }
}

/// Validate a code actually returns a quote. Mirrors `validateCode` in the original.
pub fn validate_code(market: Market, provider: Provider, code: &str) -> bool {
    let Some(url) = quote_url(provider, market, &[code.to_string()]) else {
        return false;
    };
    match get(&url, provider) {
        Ok(text) => match provider {
            Provider::Sina => {
                let Some(line) = text.lines().next() else { return false; };
                let Some(open) = line.find('"') else { return false; };
                let Some(close) = line.rfind('"') else { return false; };
                if close <= open { return false; }
                line[open + 1..close].contains(',')
            }
            Provider::Tencent => !text.starts_with("v_pv_none_match"),
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls_with_correct_prefix_and_case() {
        assert_eq!(
            quote_url(Provider::Sina, Market::AShare, &["sh600000".into()]).unwrap(),
            "https://hq.sinajs.cn/list=sh600000"
        );
        assert_eq!(
            quote_url(Provider::Sina, Market::Hk, &["00700".into()]).unwrap(),
            "https://hq.sinajs.cn/list=hk00700"
        );
        assert_eq!(
            quote_url(Provider::Sina, Market::Us, &["aapl".into()]).unwrap(),
            "https://hq.sinajs.cn/list=gb_aapl"
        );
        assert_eq!(
            quote_url(Provider::Tencent, Market::Us, &["aapl".into()]).unwrap(),
            "https://qt.gtimg.cn/q=usAAPL"
        );
        assert!(quote_url(Provider::Tencent, Market::Crypto, &["x".into()]).is_none());
    }
}

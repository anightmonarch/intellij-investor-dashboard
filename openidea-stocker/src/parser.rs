//! Quote response parsers ported from `StockerQuoteParser.kt`.
//!
//! Both providers answer with one record per line. Each line is parsed
//! independently so a single unknown/delisted symbol cannot discard the whole
//! batch — a key robustness behavior of the original plugin.

use crate::model::{Market, Provider, Quote};
use encoding_rs::GBK;

fn two_digits(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn percent(current: f64, base: f64) -> f64 {
    if base == 0.0 { 0.0 } else { two_digits((current - base) / base * 100.0) }
}

/// Decode a response body. Sina uses GBK; Tencent uses UTF-8.
pub fn decode(body: &[u8], provider: Provider) -> String {
    if provider == Provider::Sina {
        let (text, _, _) = GBK.decode(body);
        text.into_owned()
    } else {
        String::from_utf8_lossy(body).into_owned()
    }
}

/// Parse a full quote response.
pub fn parse(provider: Provider, market: Market, text: &str) -> Vec<Quote> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| match provider {
            Provider::Sina => parse_sina_line(market, line),
            Provider::Tencent => parse_tencent_line(market, line),
        })
        .collect()
}

fn parse_sina_line(market: Market, line: &str) -> Option<Quote> {
    // var hq_str_sh600000="字段名,值,值,...";
    let eq = line.find('=')?;
    let open = line.find('"')?;
    let close = line.rfind('"')?;
    if close <= open { return None; }
    // "var hq_str_sh600000=..." -> symbol starts after "var hq_str_" (11 chars)
    let symbol = line.get(11..eq)?.to_string();
    let payload = &line[open + 1..close];
    let f: Vec<&str> = std::iter::once(symbol.as_str()).chain(payload.split(',')).collect();
    let num = |i: usize| -> Option<f64> { f.get(i)?.parse().ok() };
    let strv = |i: usize| -> Option<&str> { f.get(i).copied() };

    match market {
        Market::AShare => {
            let code = strv(0)?.to_uppercase();
            let current = num(4)?;
            let base = num(3)?;
            Some(Quote {
                code, name: strv(1)?.to_string(), current,
                opening: num(2)?, close: base, low: num(6)?, high: num(5)?,
                change: two_digits(current - base), percentage: percent(current, base),
                update_at: format!("{} {}", strv(31)?, strv(32)?),
                ..Default::default()
            })
        }
        Market::Hk => {
            let code = strv(0)?.get(2..)?.to_uppercase();
            let current = num(7)?;
            let base = num(4)?;
            Some(Quote {
                code, name: strv(2)?.to_string(), current,
                opening: num(3)?, close: base, low: num(6)?, high: num(5)?,
                change: two_digits(current - base), percentage: two_digits(num(9)?),
                update_at: format!("{} {}", strv(18)?, strv(19)?),
                ..Default::default()
            })
        }
        Market::Us => {
            let code = strv(0)?.get(3..)?.to_uppercase();
            let current = num(2)?;
            let base = num(27)?;
            Some(Quote {
                code, name: strv(1)?.to_string(), current,
                opening: num(6)?, close: base, low: num(8)?, high: num(7)?,
                change: two_digits(current - base), percentage: two_digits(num(3)?),
                update_at: strv(4)?.to_string(),
                ..Default::default()
            })
        }
        Market::Crypto => {
            let code = strv(0)?.get(4..)?.to_uppercase();
            let current = num(9)?;
            let base = num(6)?;
            Some(Quote {
                code, name: strv(10)?.to_string(), current,
                opening: base, close: current, low: num(8)?, high: num(7)?,
                change: two_digits(current - base), percentage: percent(current, base),
                update_at: format!("{} {}", strv(12)?, strv(1)?),
                ..Default::default()
            })
        }
    }
}

fn parse_tencent_line(market: Market, line: &str) -> Option<Quote> {
    // v_sh600000="1~浦发银行~600000~..."
    let eq = line.find('=')?;
    let prefix_len = match market { Market::AShare => 2, _ => 4 };
    let symbol = line.get(prefix_len..eq)?;
    let open = line.find('"')?;
    let close = line.rfind('"')?;
    if close <= open { return None; }
    let payload = &line[open + 1..close];
    let f: Vec<&str> = std::iter::once(symbol).chain(payload.split('~')).collect();
    let num = |i: usize| -> Option<f64> { f.get(i)?.parse().ok() };
    let strv = |i: usize| -> Option<&str> { f.get(i).copied() };

    let current = num(4)?;
    let base = num(5)?;
    let update_at = match market {
        Market::AShare => normalize_dt(strv(31)?, "%Y%m%d%H%M%S"),
        Market::Hk => normalize_dt(strv(31)?, "%Y/%m/%d %H:%M:%S"),
        Market::Us => strv(31)?.to_string(),
        Market::Crypto => String::new(),
    };

    Some(Quote {
        code: strv(0)?.to_uppercase(),
        name: strv(2)?.to_string(),
        current,
        opening: num(6)?,
        close: base,
        low: num(35)?,
        high: num(34)?,
        change: two_digits(current - base),
        percentage: two_digits(num(33)?),
        update_at,
        ..Default::default()
    })
}

/// Normalize common datetime strings into "YYYY-MM-DD HH:MM:SS".
fn normalize_dt(raw: &str, kind: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    match kind {
        "%Y%m%d%H%M%S" if digits.len() >= 14 => format!(
            "{}-{}-{} {}:{}:{}",
            &digits[0..4], &digits[4..6], &digits[6..8],
            &digits[8..10], &digits[10..12], &digits[12..14]
        ),
        "%Y/%m/%d %H:%M:%S" if digits.len() >= 14 => format!(
            "{}-{}-{} {}:{}:{}",
            &digits[0..4], &digits[4..6], &digits[6..8],
            &digits[8..10], &digits[10..12], &digits[12..14]
        ),
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sina_a_share() {
        // Realistic shape: name,open,close,current,high,low,... time/date at 31/32
        // Sina payload starts with the name; the code is captured by the variable name.
        let mut parts = vec![""; 32];
        parts[0] = "浦发银行";
        parts[1] = "10.00"; parts[2] = "10.50"; parts[3] = "10.80";
        parts[4] = "11.00"; parts[5] = "9.90";
        parts[30] = "2026-11-26"; parts[31] = "13:20:00";
        let line = format!("var hq_str_sh600000=\"{}\";", parts.join(","));
        let q = parse(Provider::Sina, Market::AShare, &line).pop().unwrap();
        assert_eq!(q.code, "SH600000");
        assert_eq!(q.current, 10.8);
        assert_eq!(q.change, 0.3);
        assert_eq!(q.percentage, 2.86);
    }

    #[test]
    fn bad_line_does_not_kill_batch() {
        let good = "var hq_str_sh600000=\"浦发银行,10.00,10.50,10.80,11.00,9.90,,,,,,,,,,,,,,,,,,,,,,,,,,,,,2026-11-26,13:20:00\";";
        let bad = "var hq_str_sh999999=\"\";";
        let text = format!("{}\n{}", bad, good);
        let out = parse(Provider::Sina, Market::AShare, &text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "SH600000");
    }

    #[test]
    fn decodes_gbk() {
        // "浦发银行" encoded in GBK
        let gbk = b"\xc6\xd6\xb7\xa2\xd2\xf8\xd0\xd0";
        assert_eq!(decode(gbk, Provider::Sina), "浦发银行");
    }
}

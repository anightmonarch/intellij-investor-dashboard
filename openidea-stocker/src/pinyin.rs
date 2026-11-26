//! Lightweight Chinese → Pinyin conversion for stock names.
//!
//! The original plugin used pinyin4j (WITHOUT_TONE + WITH_V + LOWERCASE), then
//! capitalized each Chinese character's syllable. We use a compact map of common
//! characters found in Chinese stock names; unknown characters pass through
//! unchanged — the same graceful fallback as the original.

/// Convert a Chinese string to Pinyin, capitalizing each syllable.
/// Non-Chinese characters are passed through untouched.
pub fn to_pinyin(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match lookup(ch) {
            Some(py) => out.push_str(&py),
            None => out.push(ch),
        }
    }
    out
}

/// Capitalize the first character of a syllable: "ping" -> "Ping".
fn cap(py: &str) -> String {
    let mut it = py.chars();
    match it.next() {
        Some(first) => first.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

fn lookup(ch: char) -> Option<String> {
    MAP.iter().find(|(c, _)| *c == ch).map(|(_, p)| cap(p))
}

/// Common characters in Chinese stock names. Each entry maps one character to
/// its lowercase, toneless pinyin. Characters not listed fall back to themselves.
static MAP: &[(char, &str)] = &[
    // Banks & finance
    ('平', "ping"), ('安', "an"), ('银', "yin"), ('行', "hang"), ('招', "zhao"),
    ('商', "shang"), ('工', "gong"), ('农', "nong"), ('中', "zhong"), ('国', "guo"),
    ('建', "jian"), ('设', "she"), ('交', "jiao"), ('通', "tong"), ('邮', "you"),
    ('储', "chu"), ('财', "cai"), ('证', "zheng"), ('券', "quan"), ('保', "bao"),
    ('险', "xian"), ('信', "xin"), ('泰', "tai"), ('华', "hua"), ('夏', "xia"),
    ('大', "da"), ('陆', "lu"), ('浦', "pu"), ('发', "fa"), ('展', "zhan"),
    ('光', "guang"), ('兴', "xing"), ('业', "ye"), ('民', "min"), ('生', "sheng"),
    ('长', "chang"), ('城', "cheng"), ('市', "shi"),
    // Tech & internet
    ('腾', "teng"), ('讯', "xun"), ('阿', "a"), ('里', "li"), ('巴', "ba"),
    ('马', "ma"), ('京', "jing"), ('东', "dong"), ('小', "xiao"), ('米', "mi"),
    ('为', "wei"), ('芯', "xin"), ('科', "ke"), ('技', "ji"), ('软', "ruan"),
    ('件', "jian"), ('网', "wang"), ('易', "yi"), ('游', "you"), ('戏', "xi"),
    ('电', "dian"), ('子', "zi"), ('息', "xi"), ('数', "shu"), ('字', "zi"),
    ('智', "zhi"), ('慧', "hui"), ('人', "ren"), ('能', "neng"), ('新', "xin"),
    ('浪', "lang"), ('百', "bai"), ('度', "du"), ('搜', "sou"), ('狗', "gou"),
    ('节', "jie"), ('跳', "tiao"), ('动', "dong"),
    // Energy & resources
    ('石', "shi"), ('油', "you"), ('天', "tian"), ('然', "ran"), ('气', "qi"),
    ('煤', "mei"), ('炭', "tan"), ('力', "li"), ('源', "yuan"), ('锂', "li"),
    ('池', "chi"), ('宁', "ning"), ('德', "de"), ('时', "shi"), ('代', "dai"),
    ('阳', "yang"), ('伏', "fu"), ('风', "feng"), ('金', "jin"),
    // Consumer & healthcare
    ('茅', "mao"), ('台', "tai"), ('五', "wu"), ('粮', "liang"), ('液', "ye"),
    ('贵', "gui"), ('州', "zhou"), ('白', "bai"), ('酒', "jiu"), ('海', "hai"),
    ('尔', "er"), ('康', "kang"), ('健', "jian"), ('药', "yao"), ('医', "yi"),
    ('疗', "liao"), ('物', "wu"), ('恒', "heng"), ('瑞', "rui"), ('片', "pian"),
    ('美', "mei"), ('的', "de"), ('集', "ji"), ('团', "tuan"), ('苏', "su"),
    ('泊', "bo"), ('乳', "ru"), ('伊', "yi"), ('利', "li"), ('份', "fen"),
    ('双', "shuang"), ('汇', "hui"),
    // Industrial & manufacturing
    ('三', "san"), ('一', "yi"), ('重', "zhong"), ('机', "ji"), ('械', "xie"),
    ('汽', "qi"), ('车', "che"), ('比', "bi"), ('亚', "ya"), ('迪', "di"),
    ('吉', "ji"), ('上', "shang"), ('众', "zhong"), ('用', "yong"), ('福', "fu"),
    ('田', "tian"), ('日', "ri"), ('产', "chan"), ('现', "xian"), ('牧', "mu"),
    ('原', "yuan"), ('股', "gu"), ('有', "you"), ('限', "xian"), ('公', "gong"),
    ('司', "si"), ('控', "kong"), ('制', "zhi"), ('寿', "shou"), ('洋', "yang"),
    // Indexes & general
    ('指', "zhi"), ('沪', "hu"), ('深', "shen"), ('成', "cheng"), ('创', "chuang"),
    ('板', "ban"), ('生', "sheng"), ('道', "dao"), ('琼', "qiong"), ('斯', "si"),
    ('纳', "na"), ('达', "da"), ('克', "ke"), ('标', "biao"), ('普', "pu"),
    ('富', "fu"), ('报', "bao"), ('特', "te"), ('币', "bi"), ('以', "yi"),
    ('太', "tai"), ('坊', "fang"), ('波', "bo"), ('索', "suo"), ('拉', "la"),
    ('系', "xi"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_stock_names() {
        assert_eq!(to_pinyin("平安银行"), "PingAnYinHang");
        assert_eq!(to_pinyin("工商银行"), "GongShangYinHang");
        assert_eq!(to_pinyin("中国石油"), "ZhongGuoShiYou");
    }

    #[test]
    fn keeps_non_chinese() {
        assert_eq!(to_pinyin("AAPL"), "AAPL");
        assert_eq!(to_pinyin("腾讯控股"), "TengXunKongGu");
    }

    #[test]
    fn unknown_chinese_passes_through() {
        assert_eq!(to_pinyin("龘靐"), "龘靐");
    }
}

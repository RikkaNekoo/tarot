//! 解析用的底层字节工具：hex 解码、大端整数、BCD、GBK 文本等。

/// 把 hex 字符串解码为字节；失败返回空。
pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap_or_default()
}

/// 取 `bytes[start..end]` 大端整数（最多 8 字节）。越界返回 0。
pub fn be_uint(bytes: &[u8], start: usize, end: usize) -> u64 {
    if start >= end || end > bytes.len() {
        return 0;
    }
    let mut v = 0u64;
    for &b in &bytes[start..end] {
        v = (v << 8) | b as u64;
    }
    v
}

/// 取字节切片的十六进制（大写）。越界则尽量截取。
pub fn hex_slice(bytes: &[u8], start: usize, end: usize) -> String {
    if start >= bytes.len() {
        return String::new();
    }
    let e = end.min(bytes.len());
    hex::encode_upper(&bytes[start..e])
}

/// PBOC/交通卡余额换算：前 4 字节大端 % 0x80000000，单位分 -> 元字符串。
pub fn pboc_balance_yuan(bytes: &[u8]) -> f64 {
    let raw = be_uint(bytes, 0, 4.min(bytes.len())) % 0x8000_0000;
    raw as f64 / 100.0
}

/// GBK 解码为字符串（用于中文文本字段），去除结尾填充与不可见字符。
pub fn gbk_decode(bytes: &[u8]) -> String {
    let (cow, _, _) = encoding_rs::GBK.decode(bytes);
    cow.trim_matches(|c: char| c == '\0' || c == '\u{fffd}' || c == ' ')
        .to_string()
}

/// 去掉前导 0（用于部分卡号），全 0 则保留一个 0。
pub fn trim_leading_zeros(s: &str) -> String {
    let t = s.trim_start_matches('0');
    if t.is_empty() {
        "0".to_string()
    } else {
        t.to_string()
    }
}

/// 判断 hex 串是否为合法 BCD（每个字符都是 0-9，无 A-F）。
fn is_bcd(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// 把 YYYYMMDD 的 BCD hex（8 位）格式化为 `YYYY-MM-DD`。
///
/// 交通卡的日期为 BCD 编码（如 "20200101"）。但部分卡该字段为二进制，
/// 直接按 BCD 切分会得到 `01F4-00-00` 这类无意义值——此时保留原始 hex，
/// 由上层决定是否展示。返回值为空表示无有效日期。
pub fn fmt_date8(hex8: &str) -> String {
    if hex8.len() < 8 {
        return String::new();
    }
    let d = &hex8[0..8];
    if !is_bcd(d) {
        return String::new();
    }
    // 合法性再校验：月 01-12，日 01-31。
    let month: u32 = d[4..6].parse().unwrap_or(0);
    let day: u32 = d[6..8].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return String::new();
    }
    format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
}

/// 把 YYYYMMDDhhmmss 的 BCD hex（14 位）格式化为 `YYYY-MM-DD HH:MM:SS`。
/// 非法 BCD 返回原始 hex 以便排查。
pub fn fmt_timestamp14(hex14: &str) -> String {
    if hex14.len() < 14 || !is_bcd(&hex14[0..14]) {
        return hex14.to_string();
    }
    let d = &hex14[0..14];
    format!(
        "{}-{}-{} {}:{}:{}",
        &d[0..4],
        &d[4..6],
        &d[6..8],
        &d[8..10],
        &d[10..12],
        &d[12..14]
    )
}

/// 把 HHMMSS 的 BCD hex（6 位）格式化为 `HH:MM:SS`。非法 BCD 返回空。
pub fn fmt_time6(hex6: &str) -> String {
    if hex6.len() < 6 {
        return String::new();
    }
    let t = &hex6[0..6];
    if !is_bcd(t) {
        return String::new();
    }
    let h: u32 = t[0..2].parse().unwrap_or(99);
    let m: u32 = t[2..4].parse().unwrap_or(99);
    let s: u32 = t[4..6].parse().unwrap_or(99);
    if h > 23 || m > 59 || s > 59 {
        return String::new();
    }
    format!("{}:{}:{}", &t[0..2], &t[2..4], &t[4..6])
}

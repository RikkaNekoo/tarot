//! MRZ（机读区）通用工具：字段清洗、日期与性别格式化、姓名拆分。
//!
//! 护照（TD3）与港澳通行证（TD1，中国排布）共用这些原语，
//! 各自的行列布局解析见 `passport` / `eep` 子模块。

/// MRZ 填充符。
pub const FILLER: char = '<';

/// 去掉 MRZ 填充符，其余填充符视作分隔空格，两端修剪。
pub fn field(s: &str) -> String {
    s.trim_matches(FILLER).replace(FILLER, " ").trim().to_string()
}

/// 拆分姓名字段（姓与名以两个填充符分隔），返回 (姓, 名)。
pub fn split_name(field_str: &str) -> (String, String) {
    let sep: String = [FILLER, FILLER].iter().collect();
    let trimmed = field_str.trim_end_matches(FILLER);
    match trimmed.split_once(&sep) {
        Some((sur, giv)) => (
            sur.replace(FILLER, " ").trim().to_string(),
            giv.replace(FILLER, " ").trim().to_string(),
        ),
        None => (
            trimmed.replace(FILLER, " ").trim().to_string(),
            String::new(),
        ),
    }
}

/// MRZ 日期 YYMMDD -> YYYY-MM-DD。世纪以 50 为阈值粗略推断。
pub fn date(s: &str) -> String {
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let yy: u32 = s[0..2].parse().unwrap_or(0);
    let mm = &s[2..4];
    let dd = &s[4..6];
    let year = if yy <= 50 { 2000 + yy } else { 1900 + yy };
    format!("{year}-{mm}-{dd}")
}

/// 性别码 M/F/其它 -> 中文。
pub fn sex(s: &str) -> String {
    match s {
        "M" => "男".into(),
        "F" => "女".into(),
        _ => "未指定".into(),
    }
}

//! EMV 卡解析。综合 FCI 与各记录中的标准 EMV TLV 提取信息：
//! 应用标签、PAN（tag 5A，回退 Track2）、生效/失效日期、PAN 序列号、
//! 发卡国家、应用货币、版本号，并展示 ATC / PIN 重试次数。

use super::model::ParsedCard;
use super::tlv;
use super::util::*;
use tarot_core::RawCardData;

/// 品牌标识 -> 可读名。
fn brand_display(brand: &str) -> String {
    match brand {
        "UnionPay-Debit" => "银联借记",
        "UnionPay-Credit" => "银联贷记",
        "UnionPay-SecuredCredit" => "银联准贷记",
        "Visa" => "Visa",
        "MasterCard" | "MasterCard-China" => "MasterCard",
        "AMEX" | "AMEX-China" => "American Express",
        "JCB" => "JCB",
        "Discover" => "Discover",
        other => other,
    }
    .to_string()
}

/// ISO 3166-1 数字国家码 -> 中文名（常见值）。
fn country_name(code: &str) -> Option<&'static str> {
    match code {
        "156" => Some("中国"),
        "344" => Some("中国香港"),
        "446" => Some("中国澳门"),
        "158" => Some("中国台湾"),
        "840" => Some("美国"),
        "826" => Some("英国"),
        "392" => Some("日本"),
        "410" => Some("韩国"),
        "702" => Some("新加坡"),
        "036" => Some("澳大利亚"),
        _ => None,
    }
}

/// ISO 4217 数字货币码 -> (代码, 符号)。
fn currency_name(code: &str) -> Option<&'static str> {
    match code {
        "156" => Some("CNY 人民币"),
        "344" => Some("HKD 港币"),
        "446" => Some("MOP 澳门元"),
        "840" => Some("USD 美元"),
        "978" => Some("EUR 欧元"),
        "826" => Some("GBP 英镑"),
        "392" => Some("JPY 日元"),
        "702" => Some("SGD 新加坡元"),
        _ => None,
    }
}

/// BCD 编码的 n3 数字字段（如国家/货币码 0156）-> 去前导零的十进制串 "156"。
/// EMV 中这类码是压缩 BCD：每半字节一位十进制数字。
fn bcd_n3(bytes: &[u8]) -> String {
    let s = hex::encode(bytes); // 如 "0156"
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// EMV 日期字段 YYMMDD 的 BCD hex（6 位）-> `20YY-MM-DD`。非法返回原始 hex。
fn fmt_emv_date(hex6: &str) -> String {
    if hex6.len() < 6 || !hex6.chars().all(|c| c.is_ascii_digit()) {
        return hex6.to_string();
    }
    let yy = &hex6[0..2];
    let mm = &hex6[2..4];
    let dd = &hex6[4..6];
    format!("20{yy}-{mm}-{dd}")
}

/// 收集所有含 EMV TLV 的字节来源：FCI + GPO + 各记录。
fn collect_sources(raw: &RawCardData) -> Vec<Vec<u8>> {
    let mut sources: Vec<Vec<u8>> = Vec::new();
    for key in ["emv_app_fci", "ppse_fci", "emv_gpo"] {
        if let Some(v) = raw.get(key) {
            sources.push(hex_to_bytes(v));
        }
    }
    for (k, v) in &raw.raw_fields {
        if k.starts_with("emv_rec_") {
            sources.push(hex_to_bytes(v));
        }
    }
    sources
}

/// 在所有来源中查找首个匹配 tag 的值（hex 大写）。
fn find_value(sources: &[Vec<u8>], tag: &str) -> Option<Vec<u8>> {
    for bytes in sources {
        let nodes = tlv::parse(bytes);
        if let Some(t) = tlv::find(&nodes, tag) {
            if !t.value.is_empty() {
                return Some(t.value.clone());
            }
        }
    }
    None
}

pub fn parse(raw: &RawCardData) -> ParsedCard {
    let brand = raw.get("emv_brand").unwrap_or("");
    let mut card = ParsedCard::new(if brand.is_empty() {
        "EMV 银行卡".to_string()
    } else {
        brand_display(brand)
    });

    let sources = collect_sources(raw);

    if let Some(aid) = raw.get("emv_aid") {
        card.add_field("AID", aid.to_string());
    }

    // 应用标签（tag 50），如 "Debit Mastercard"。
    if let Some(v) = find_value(&sources, "50") {
        let label = String::from_utf8_lossy(&v).trim().to_string();
        if !label.is_empty() {
            card.add_field("应用标签", label);
        }
    }

    parse_pan_and_dates(&sources, &mut card);
    parse_issuer_fields(&sources, &mut card);
    parse_get_data_fields(raw, &mut card);

    card
}

/// 解析 PAN（tag 5A，回退 Track2 tag 57）、生效/失效日期、PAN 序列号。
fn parse_pan_and_dates(sources: &[Vec<u8>], card: &mut ParsedCard) {
    // PAN：优先 tag 5A（BCD，尾部 F 填充）。
    if let Some(v) = find_value(sources, "5A") {
        let pan = hex::encode_upper(&v);
        card.number = Some(pan.trim_end_matches(['F', 'f']).to_string());
    }

    // Track2（tag 57）：作为 PAN 回退，并提取有效期。
    if let Some(v) = find_value(sources, "57") {
        let t2 = hex::encode_upper(&v);
        parse_track2(&t2, card);
    } else if card.number.is_none() {
        card.notes.push("未找到 PAN / Track2（可能需 PDOL）".into());
    }

    // 失效日期 5F24、生效日期 5F25（YYMMDD BCD）。
    if let Some(v) = find_value(sources, "5F24") {
        card.add_field("失效日期", fmt_emv_date(&hex::encode_upper(&v)));
    }
    if let Some(v) = find_value(sources, "5F25") {
        card.add_field("生效日期", fmt_emv_date(&hex::encode_upper(&v)));
    }
    // PAN 序列号 5F34。
    if let Some(v) = find_value(sources, "5F34") {
        card.add_field("PAN 序列号", hex::encode_upper(&v));
    }
}

/// 解析发卡行/应用相关字段：持卡人名、发卡国家、应用货币、版本号。
fn parse_issuer_fields(sources: &[Vec<u8>], card: &mut ParsedCard) {
    // 持卡人名 5F20（可能 GBK 编码）。
    if let Some(v) = find_value(sources, "5F20") {
        let name = gbk_decode(&v);
        if !name.is_empty() {
            card.add_field("持卡人", name);
        }
    }
    // 发卡行国家代码 5F28（BCD，n3 格式 ISO 3166-1，如 0156→156）。
    if let Some(v) = find_value(sources, "5F28") {
        let code = bcd_n3(&v);
        let disp = country_name(&code)
            .map(|n| format!("{n}（{code}）"))
            .unwrap_or(code);
        card.add_field("发卡国家", disp);
    }
    // 应用货币代码 9F42（BCD，n3 格式 ISO 4217，如 0156→156）。
    if let Some(v) = find_value(sources, "9F42") {
        let code = bcd_n3(&v);
        let disp = currency_name(&code).map(|s| s.to_string()).unwrap_or(code);
        card.add_field("应用货币", disp);
    }
    // 应用版本号 9F08。
    if let Some(v) = find_value(sources, "9F08") {
        card.add_field("应用版本号", hex::encode_upper(&v));
    }
}

/// 解析 GET DATA 得到的字段：ATC、PIN 剩余重试次数。
fn parse_get_data_fields(raw: &RawCardData, card: &mut ParsedCard) {
    if let Some(atc) = raw.get("emv_atc") {
        let nodes = tlv::parse(&hex_to_bytes(atc));
        if let Some(t) = tlv::find(&nodes, "9F36") {
            card.add_field(
                "应用交易计数(ATC)",
                be_uint(&t.value, 0, t.value.len()).to_string(),
            );
        }
    }
    if let Some(pin) = raw.get("emv_pin_retry") {
        let nodes = tlv::parse(&hex_to_bytes(pin));
        if let Some(t) = tlv::find(&nodes, "9F17") {
            card.add_field("PIN 剩余重试", be_uint(&t.value, 0, t.value.len()).to_string());
        }
    }
}

/// 解析 Track2 Equivalent Data。格式 `PAN D YYMM SC ...`，D 为分隔符。
/// PAN 仅在 tag 5A 缺失时作为回退填入；有效期总是提取。
fn parse_track2(t2: &str, card: &mut ParsedCard) {
    if let Some(pos) = t2.find(['D', 'd']) {
        if card.number.is_none() {
            card.number = Some(t2[..pos].to_string());
        }
        let rest = &t2[pos + 1..];
        if rest.len() >= 4 && !card.fields.iter().any(|(k, _)| k == "失效日期") {
            let yy = &rest[0..2];
            let mm = &rest[2..4];
            card.add_field("有效期", format!("20{yy}-{mm}"));
        }
    } else if card.number.is_none() {
        card.number = Some(t2.trim_end_matches(['F', 'f']).to_string());
    }
}

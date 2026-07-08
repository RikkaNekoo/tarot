//! EMV 卡解析。从 GPO/记录中提取 Track2（tag 57）得到 PAN 与有效期，
//! 品牌来自后端 `emv_brand`，并展示 ATC / PIN 重试次数。

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

pub fn parse(raw: &RawCardData) -> ParsedCard {
    let brand = raw.get("emv_brand").unwrap_or("");
    let mut card = ParsedCard::new(if brand.is_empty() {
        "EMV 银行卡".to_string()
    } else {
        brand_display(brand)
    });

    if let Some(aid) = raw.get("emv_aid") {
        card.add_field("AID", aid.to_string());
    }

    // 收集所有可能含 Track2 的 TLV 来源：GPO + 各记录。
    let mut track2: Option<String> = None;
    let mut sources: Vec<Vec<u8>> = Vec::new();
    if let Some(g) = raw.get("emv_gpo") {
        sources.push(hex_to_bytes(g));
    }
    for (k, v) in &raw.raw_fields {
        if k.starts_with("emv_rec_") {
            sources.push(hex_to_bytes(v));
        }
    }

    for bytes in &sources {
        let nodes = tlv::parse(bytes);
        // Track2 Equivalent Data（tag 57）。
        if track2.is_none() {
            if let Some(t) = tlv::find(&nodes, "57") {
                track2 = Some(hex::encode_upper(&t.value));
            }
        }
        // 持卡人名 5F20。
        if let Some(t) = tlv::find(&nodes, "5F20") {
            let name = gbk_decode(&t.value);
            if !name.is_empty() {
                card.add_field("持卡人", name);
            }
        }
    }

    // 解析 Track2：PAN = 'D' 前部分；有效期 YYMM 在 'D' 后 4 位。
    if let Some(t2) = &track2 {
        parse_track2(t2, &mut card);
    } else {
        card.notes.push("未找到 Track2（可能需 PDOL）".into());
    }

    // ATC。
    if let Some(atc) = raw.get("emv_atc") {
        let nodes = tlv::parse(&hex_to_bytes(atc));
        if let Some(t) = tlv::find(&nodes, "9F36") {
            card.add_field("应用交易计数(ATC)", be_uint(&t.value, 0, t.value.len()).to_string());
        }
    }
    // PIN 重试次数。
    if let Some(pin) = raw.get("emv_pin_retry") {
        let nodes = tlv::parse(&hex_to_bytes(pin));
        if let Some(t) = tlv::find(&nodes, "9F17") {
            card.add_field("PIN 剩余重试", be_uint(&t.value, 0, t.value.len()).to_string());
        }
    }

    card
}

/// 解析 Track2 Equivalent Data。格式 `PAN D YYMM SC ...`，D 为分隔符（十六进制 D）。
fn parse_track2(t2: &str, card: &mut ParsedCard) {
    // 分隔符可能是 'D' 或 'F' 填充；找第一个 'D'。
    if let Some(pos) = t2.find(|c| c == 'D' || c == 'd') {
        let pan = &t2[..pos];
        card.number = Some(pan.to_string());
        let rest = &t2[pos + 1..];
        if rest.len() >= 4 {
            let yy = &rest[0..2];
            let mm = &rest[2..4];
            card.add_field("有效期", format!("20{yy}-{mm}"));
        }
    } else {
        // 无分隔符：整体作为 PAN（去尾部 F 填充）。
        card.number = Some(t2.trim_end_matches(['F', 'f']).to_string());
    }
}
//! Mifare 系列解析：Ultralight/NTAG（`ul_data`）、Classic（`classic_data`）、
//! DESFire（`desfire_version_N`）。这些卡多为存储卡，展示 UID、型号与原始数据摘要。

use super::model::ParsedCard;
use super::util::*;
use tarot_core::RawCardData;

/// 解析 Mifare Ultralight / NTAG。UID 在页 0-1 前 7 字节。
pub fn parse_ultralight(raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new("Mifare Ultralight / NTAG");
    if let Some(hex) = raw.get("ul_data") {
        let bytes = hex_to_bytes(hex);
        // UID：页0 前3字节(含BCC0) + 页1 4字节，取 7 字节序列号常规布局。
        if bytes.len() >= 9 {
            let mut uid = Vec::new();
            uid.extend_from_slice(&bytes[0..3]);
            uid.extend_from_slice(&bytes[4..8]);
            card.number = Some(hex::encode_upper(&uid));
        }
        card.add_field("已读字节", (bytes.len()).to_string());
        card.add_field("原始数据", truncate_hex(hex, 64));
    } else {
        card.notes.push("未读到 Ultralight 数据".into());
    }
    card
}

/// 解析 Mifare Classic。展示读到的扇区数据摘要。
pub fn parse_classic(raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new("Mifare Classic");
    if let Some(hex) = raw.get("classic_data") {
        let bytes = hex_to_bytes(hex);
        // 块0 前 4 字节通常为 UID。
        if bytes.len() >= 4 {
            card.number = Some(hex_slice(&bytes, 0, 4));
        }
        card.add_field("已读字节", bytes.len().to_string());
        card.add_field("原始数据", truncate_hex(hex, 64));
    } else {
        card.notes.push("未读到 Classic 数据（密钥可能不匹配）".into());
    }
    card
}

/// 解析 DESFire 版本信息。硬件版本第 1 帧：厂商/类型/子类型/版本/容量。
pub fn parse_desfire(raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new("Mifare DESFire");
    if let Some(hex) = raw.get("desfire_version_1") {
        let b = hex_to_bytes(hex);
        if b.len() >= 7 {
            card.add_field("厂商", format!("{:02X}", b[0]));
            card.add_field("类型", format!("{:02X}", b[1]));
            let storage = b[5];
            // 容量 2^(n/2)：DESFire 用 (n>>1) 编码字节数。
            let bytes = 1u32 << (storage >> 1);
            card.add_field("存储容量", format!("~{} bytes", bytes));
        }
    }
    // 生产信息在第 3 帧（若有）：BCD 周/年。
    if let Some(hex) = raw.get("desfire_version_3") {
        let b = hex_to_bytes(hex);
        if b.len() >= 7 {
            let week = b[5];
            let year = b[6];
            card.add_field("生产", format!("第 {:02X} 周 / 20{:02X} 年", week, year));
        }
    }
    // UID 常在生产信息帧前 7 字节。
    if let Some(hex) = raw.get("desfire_version_3") {
        let b = hex_to_bytes(hex);
        if b.len() >= 7 {
            card.number = Some(hex_slice(&b, 0, 7));
        }
    }
    card
}

/// 截断 hex 展示，超长加省略号。
fn truncate_hex(hex: &str, max: usize) -> String {
    if hex.len() > max {
        format!("{}…", &hex[..max])
    } else {
        hex.to_string()
    }
}
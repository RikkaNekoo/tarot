//! 旅行证件解析：DG1（MRZ 机读区）、DG2（人脸）、DG11/DG12（附加信息）。
//!
//! 后端已完成 BAC 与安全通道，把解密后的各 DG 明文存入 `raw_fields`
//! （键 `passport_dg1` / `passport_dg2` 等），此处只解析明文语义。
//!
//! 护照与港澳通行证读取流程一致，究竟是哪种由 MRZ 判定：
//! - 护照：TD3（88 字符），文档码首字符 `P`；
//! - 通行证：TD1 变体（90 字符），文档码首字符 `C`。
//!
//! 规范：ICAO Doc 9303 Part 4（MRZ）/ Part 10（LDS）。

pub mod eep;
pub mod mrz;
pub mod passport;

use super::model::ParsedCard;
use super::tlv;
use super::util::hex_to_bytes;
use tarot_core::RawCardData;

/// 证件类别（由 MRZ 判定）。
enum DocKind {
    Passport,
    Eep,
    Unknown,
}

/// 解析旅行证件，产出可读卡片信息。
pub fn parse(raw: &RawCardData) -> ParsedCard {
    // 先取 MRZ 判定类别，决定卡片标题与字段布局。
    let mrz_clean = raw
        .get("passport_dg1")
        .and_then(|dg1| extract_mrz(&hex_to_bytes(dg1)))
        .map(|m| m.chars().filter(|c| !c.is_whitespace()).collect::<String>());

    let kind = detect_kind(mrz_clean.as_deref());
    let title = match kind {
        DocKind::Passport => "电子护照",
        DocKind::Eep => "电子往来港澳通行证",
        DocKind::Unknown => "旅行证件",
    };

    let mut card = ParsedCard::new(title);
    card.currency = String::new();

    match mrz_clean.as_deref() {
        Some(clean) => match kind {
            DocKind::Passport => passport::parse(&mut card, clean),
            DocKind::Eep => eep::parse(&mut card, clean),
            DocKind::Unknown => {
                card.notes
                    .push(format!("MRZ 长度异常({})，无法解析字段", clean.len()));
                card.add_field("MRZ", clean.to_string());
            }
        },
        None => card.notes.push("未读到 DG1（MRZ）".into()),
    }

    // DG11：附加个人信息（含 UTF-8 中文姓名）。
    if let Some(dg11) = raw.get("passport_dg11") {
        fill_from_dg11(&mut card, &hex_to_bytes(dg11));
    }

    // DG2：人脸图像信息
    if let Some(dg2) = raw.get("passport_dg2") {
        let bytes = hex_to_bytes(dg2);
        match face_image_info(&bytes) {
            Some((fmt, size)) => card.add_field("人脸图像", format!("{fmt}，{size} 字节")),
            None => card.add_field("人脸图像", "存在（格式未识别）"),
        }
    }

    if raw.get("passport_sod").is_some() {
        card.add_field("安全对象", "SOD 已读取（含签名，未做 PA 验证）");
    }

    card
}

/// 由 MRZ 长度与文档码判定证件类别。
fn detect_kind(mrz: Option<&str>) -> DocKind {
    match mrz {
        Some(s) if s.len() == 88 => DocKind::Passport,
        Some(s) if s.len() == 90 => DocKind::Eep,
        _ => DocKind::Unknown,
    }
}

/// 从 DG1（`61` → `5F1F`）提取 MRZ 文本。
fn extract_mrz(bytes: &[u8]) -> Option<String> {
    let nodes = tlv::parse(bytes);
    let node = tlv::find(&nodes, "5F1F")?;
    let s: String = node.value.iter().map(|&b| b as char).collect();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 解析 DG11（`6B` 包裹）：`5F0E` 全名(UTF-8 中文)。
fn fill_from_dg11(card: &mut ParsedCard, bytes: &[u8]) {
    let nodes = tlv::parse(bytes);
    if let Some(n) = tlv::find(&nodes, "5F0E") {
        let name = String::from_utf8_lossy(&n.value).trim().to_string();
        if !name.is_empty() {
            card.add_field("中文姓名", name);
        }
    }
}

/// 探测 DG2 内的人脸图像格式与大小。
/// DG2 内层为 ISO 19794-5，图像通常为 JPEG(FFD8) 或 JPEG2000。
fn face_image_info(bytes: &[u8]) -> Option<(&'static str, usize)> {
    if let Some(pos) = find_subslice(bytes, &[0xFF, 0xD8, 0xFF]) {
        return Some(("JPEG", bytes.len() - pos));
    }
    if let Some(pos) = find_subslice(bytes, &[0xFF, 0x4F, 0xFF, 0x51]) {
        return Some(("JPEG2000", bytes.len() - pos));
    }
    if let Some(pos) = find_subslice(bytes, &[0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50]) {
        return Some(("JPEG2000", bytes.len() - pos));
    }
    None
}

/// 在字节序列中查找子序列首次出现位置。
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
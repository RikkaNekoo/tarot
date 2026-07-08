//! 前端解析层：把后端抓到的 `RawCardData`（原始 hex）按 card_type / sub_cards
//! 分派到各卡种解析器，产出人类可读的 `ParsedResult`。

pub mod codes;
pub mod emv;
pub mod felica;
pub mod mifare;
pub mod model;
pub mod tlv;
pub mod tmoney;
pub mod transit;
pub mod traveldoc;
pub mod util;

pub use model::{ParsedCard, ParsedResult};

use tarot_core::RawCardData;

/// 主入口：解析整张卡（含叠加子卡）。
pub fn parse(raw: &RawCardData) -> ParsedResult {
    let mut result = ParsedResult {
        card_type: raw.card_type.clone(),
        atr: raw.atr.clone(),
        cards: Vec::new(),
    };

    // 单一物理层卡（Mifare/Felica）直接按 card_type 分派。
    match raw.card_type.as_str() {
        "MifareUltralight" => {
            result.cards.push(mifare::parse_ultralight(raw));
            return result;
        }
        "MifareClassic" => {
            result.cards.push(mifare::parse_classic(raw));
            return result;
        }
        "Octopus" | "Felica" => {
            result.cards.push(felica::parse(raw));
            return result;
        }
        "TravelDoc" => {
            result.cards.push(traveldoc::parse(raw));
            return result;
        }
        _ => {}
    }

    // Type A / B：按 sub_cards 逐一解析。
    for sub in &raw.sub_cards {
        let card = parse_sub(sub, raw);
        result.cards.push(card);
    }

    // 未识别到任何子卡但 card_type 有内容：给出占位卡。
    if result.cards.is_empty() {
        let mut c = ParsedCard::new(if raw.card_type == "Unknown" {
            "未识别卡片".to_string()
        } else {
            raw.card_type.clone()
        });
        if raw.raw_fields.is_empty() {
            c.notes.push("未抓到任何数据".into());
        } else {
            c.notes.push("有原始数据但无匹配解析器".into());
        }
        result.cards.push(c);
    }

    result
}

/// 分派单张子卡到对应解析器。
fn parse_sub(sub: &str, raw: &RawCardData) -> ParsedCard {
    match sub {
        "EMV" => emv::parse(raw),
        "Octopus" => felica::parse(raw),
        "MifareDESFire" => mifare::parse_desfire(raw),
        "TMoney" => tmoney::parse(raw),
        "ShenzhenTong" | "WuhanTong" | "LingnanPass" | "CityUnion" | "TUnion" | "MacauPass"
        | "BMAC" => transit::parse(sub, raw),
        other => {
            // EMV 品牌型 card_type（如 "EMV:Visa"）也归 EMV。
            if raw.card_type.starts_with("EMV") {
                emv::parse(raw)
            } else {
                let mut c = ParsedCard::new(other.to_string());
                c.notes.push("无专用解析器".into());
                c
            }
        }
    }
}

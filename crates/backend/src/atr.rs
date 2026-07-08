//! 从 ATR（Answer To Reset）推断卡片协议层，替代 nfsee 中来自 NFC 底层的
//! `tag.standard`。
//!
//! 非接卡经 PC/SC 时，ATR 遵循 PC/SC Part 3 的合成格式：
//! `3B 8n 80 01 <历史字节...> <TCK>`，其中历史字节内嵌 SS(标准) 与 卡名。
//! 关键字段：
//! - 若历史字节含 `80 4F 0C ... A0 00 00 03 06`（RID）后跟 SS 字节与卡名。
//! - SS = 03 -> ISO14443A，SS = 05/0B -> ISO14443B（不同厂商略有差异）。
//! - 卡名 `00 01` = Mifare Classic 1K，`00 02` = 4K，`00 03` = Ultralight，
//!   `F0 04` = Topaz，`00 3A` = Ultralight C 等；FeliCa 由专用 ATR 段标识。

use tarot_core::CardStandard;

/// 根据 ATR 字节推断协议层。尽力而为，无法判定时返回 `Iso14443A`（最常见）。
pub fn detect(atr: &[u8]) -> CardStandard {
    // 寻找 PC/SC 合成 ATR 的应用标识段：A0 00 00 03 06（PC/SC RID）
    if let Some(pos) = find_subslice(atr, &[0xA0, 0x00, 0x00, 0x03, 0x06]) {
        // 紧随 RID 的是 SS(1) + 卡名(2)
        let ss_idx = pos + 5;
        if let (Some(&ss), Some(name)) = (atr.get(ss_idx), atr.get(ss_idx + 1..ss_idx + 3)) {
            return classify(ss, name);
        }
    }
    // 无法解析则默认按 Type A CPU 卡处理（覆盖大部分交通卡/EMV）。
    CardStandard::Iso14443A
}

/// 根据 SS（标准字节）与卡名映射到协议层。
fn classify(ss: u8, name: &[u8]) -> CardStandard {
    // 卡名段识别 Mifare 家族
    match name {
        [0x00, 0x01] | [0x00, 0x02] => return CardStandard::MifareClassic, // 1K/4K
        [0x00, 0x03] | [0x00, 0x3A] => return CardStandard::MifareUltralight,
        [0x00, 0x3B] => return CardStandard::Felica, // FeliCa（部分读卡器映射）
        _ => {}
    }
    // SS 字节区分 A/B
    match ss {
        0x03 => CardStandard::Iso14443A,
        0x05 | 0x0B | 0x0C => CardStandard::Iso14443B,
        _ => CardStandard::Iso14443A,
    }
}

/// 在 `haystack` 中查找 `needle` 首次出现的位置。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}
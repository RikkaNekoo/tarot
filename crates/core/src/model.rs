//! 卡片原始数据模型与传输抽象。
//!
//! 后端只填充「原始字节」相关字段（raw APDU 交互结果），
//! 语义解析（卡号、余额、交易记录）由前端根据 `card_type` 完成。

use crate::apdu::{Apdu, ApduResponse};
use crate::error::Result;

/// 卡片物理/协议层类别，替代 nfsee 中来自 NFC 底层的 `tag.standard`。
/// PC/SC 下由 ATR 解析推断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardStandard {
    /// ISO 14443-4 Type A（大多数 CPU 卡 / EMV / 交通卡）。
    Iso14443A,
    /// ISO 14443-4 Type B（部分银行卡）。
    Iso14443B,
    /// Mifare Classic。
    MifareClassic,
    /// Mifare Ultralight / NTAG。
    MifareUltralight,
    /// FeliCa（如 Octopus）。
    Felica,
    /// 未知。
    Unknown,
}

/// 一次 APDU 往返的追踪记录，供前端「APDU 追踪器」展示。
#[derive(Debug, Clone)]
pub struct ApduTrace {
    /// 发送的命令十六进制。
    pub tx: String,
    /// 返回的完整响应十六进制（含 SW）。
    pub rx: String,
    /// 可选的人类可读说明（如 "SELECT PPSE"）。
    pub note: Option<String>,
}

/// 后端读卡的最终产物：卡片类型 + 原始字节集合 + APDU 历史。
///
/// `raw_fields` 用键值形式承载各步骤抓到的原始十六进制数据，
/// 键名与 nfsee 中的语义对应（如 `"ppse_fci"`、`"balance"`、`"file_15"`），
/// 前端据此解析。
#[derive(Debug, Clone, Default)]
pub struct RawCardData {
    /// 识别出的卡片类型标识（如 "PPSE:Visa"、"ShenzhenTong"）。
    pub card_type: String,
    /// ATR（Answer To Reset）原始字节的十六进制。
    pub atr: String,
    /// 各步骤抓取的原始数据，键 -> 十六进制字符串。
    pub raw_fields: Vec<(String, String)>,
    /// 若为叠加卡，记录各子卡类型。
    pub sub_cards: Vec<String>,
    /// 完整 APDU 交互历史。
    pub apdu_history: Vec<ApduTrace>,
}

impl RawCardData {
    /// 追加一个原始字段。
    pub fn put(&mut self, key: impl Into<String>, hex_value: impl Into<String>) {
        self.raw_fields.push((key.into(), hex_value.into()));
    }

    /// 按键查找原始字段。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.raw_fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// 传输抽象：任何能收发 APDU 的通道都实现它。
///
/// 后端的 PC/SC 连接实现此 trait；卡种探测/读取逻辑依赖它而非具体实现，
/// 便于测试（可注入 mock）与前端调用。
pub trait Transceiver {
    /// 发送一条 APDU 并返回解析后的响应。实现需记录 APDU 历史。
    fn transceive(&mut self, apdu: &Apdu) -> Result<ApduResponse>;

    /// 便捷方法：直接用十六进制字符串发送。
    fn transceive_hex(&mut self, hex: &str) -> Result<ApduResponse> {
        let apdu = Apdu::from_hex(hex)?;
        self.transceive(&apdu)
    }
}
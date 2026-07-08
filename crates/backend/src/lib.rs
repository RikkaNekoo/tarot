//! `tarot-backend`：PC/SC 读卡后端。
//!
//! 职责边界：只与卡片进行原生 APDU 交互并抓取原始字节，不做业务解析
//! （TLV / 余额 / 交易记录解析由前端完成）。
//!
//! 两种使用方式：
//! - 库模式：调用 [`read_once`] 或使用 [`reader::PcscManager`] + [`cards::read_card`]。
//! - CLI 模式：见 `src/bin/cli.rs`。

pub mod atr;
pub mod cards;
pub mod reader;

use reader::PcscManager;
use tarot_core::{PassportKey, RawCardData, Result};

/// 一次完整读卡：自动选第一个读卡器、连接、按 ATR 推断协议层并读取。
///
/// 返回原始卡数据（含 APDU 历史）。供前端一键调用。
pub fn read_once() -> Result<RawCardData> {
    let mgr = PcscManager::new()?;
    let reader = mgr.first_reader()?;
    read_from_reader(&mgr, &reader)
}

/// 从指定读卡器读取一次。
pub fn read_from_reader(mgr: &PcscManager, reader: &str) -> Result<RawCardData> {
    let mut session = mgr.connect(reader)?;
    let atr = session.atr_hex()?;
    let atr_bytes = session.atr()?;
    let standard = atr::detect(&atr_bytes);

    let mut data = cards::read_card(&mut session, standard, &atr)?;
    // 把会话累积的 APDU 历史并入结果。
    data.apdu_history = session.into_history();
    Ok(data)
}

/// 从指定读卡器读取旅行证件（电子护照 / 港澳通行证，需要 MRZ 三要素）。
///
/// 与普通读卡不同：旅行证件需外部 MRZ 密钥并在后端完成 BAC + 安全消息通道，
/// 解密后的各数据组明文写入返回的 `RawCardData.raw_fields`。
/// 护照与通行证读取流程一致，究竟是哪种由前端解析 MRZ 判定。
pub fn read_traveldoc_from_reader(
    mgr: &PcscManager,
    reader: &str,
    key: &PassportKey,
) -> Result<RawCardData> {
    let mut session = mgr.connect(reader)?;
    let atr = session.atr_hex()?;
    let mut data = RawCardData {
        atr,
        ..Default::default()
    };
    cards::traveldoc::read_traveldoc(&mut session, key, &mut data)?;
    data.apdu_history = session.into_history();
    Ok(data)
}

/// 列出所有读卡器名称。
pub fn list_readers() -> Result<Vec<String>> {
    PcscManager::new()?.list_readers()
}

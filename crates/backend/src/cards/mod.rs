//! 卡种探测与读取编排，对应 nfsee `read.js` 的 `ReadAnyCard`。
//!
//! 后端只抓原始字节；此处按物理层与 AID 探测链决定读哪些数据。

pub mod aids;
pub mod emv;
pub mod helpers;
pub mod transit;
pub mod traveldoc;

use tarot_core::{CardStandard, RawCardData, Result, Transceiver};

/// 主入口：根据 ATR 推断的协议层，跑对应的读取流程。
///
/// 返回填充好原始字节的 [`RawCardData`]（含 `card_type`、`raw_fields`、`sub_cards`）。
pub fn read_card<T: Transceiver>(
    tx: &mut T,
    reader_name: &str,
    standard: CardStandard,
    atr_hex: &str,
) -> Result<RawCardData> {
    let mut data = RawCardData {
        atr: atr_hex.to_string(),
        card_type: "Unknown".into(),
        ..Default::default()
    };

    match standard {
        CardStandard::Iso14443B => {
            // Type B 卡：走 EMV(PPSE) 探测（部分银行卡为 Type B）。
            if emv::probe_ppse(tx, &mut data)? {
                data.sub_cards.push("EMV".to_string());
                data.card_type = "EMV".into();
            }
        }
        CardStandard::Iso14443A => {
            read_type_a(tx, &mut data)?;
        }
        CardStandard::MifareUltralight => {
            mifare::read_ultralight(tx, &mut data)?;
        }
        CardStandard::MifareClassic => {
            mifare::read_classic(tx, &mut data)?;
        }
        CardStandard::Felica => {
            felica::read_octopus(tx, reader_name, &mut data)?;
        }
        CardStandard::Unknown => {}
    }

    Ok(data)
}

pub mod felica;
pub mod mifare;

/// Type A 探测链：逐一尝试各交通卡 AID，再尝试 EMV(PPSE)。
/// 与 nfsee 一样支持叠加卡（多张卡共存于一枚芯片）。
fn read_type_a<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    // 北京一卡通：直接读 0x84 文件（无需 SELECT）
    transit::probe_beijing(tx, data)?;

    // DESFire GetVersion
    mifare::probe_desfire(tx, data)?;

    // 遍历交通/支付卡 AID 探测链
    for entry in aids::TYPE_A_CHAIN {
        if helpers::select_aid(tx, entry.aid, data, entry.key)? {
            transit::read_by_name(tx, entry.name, data)?;
            data.sub_cards.push(entry.name.to_string());
        }
    }

    // Macau Pass 备用路径（AMTJAVACARD 容器）
    transit::probe_macau_fallback(tx, data)?;

    // PPSE（EMV）放最后，iOS 上易失败
    if emv::probe_ppse(tx, data)? {
        data.sub_cards.push("EMV".to_string());
    }

    // 多协议卡直接用命中的协议列表命名，避免额外的 CombinedCard 概念。
    if data.sub_cards.len() > 1 {
        data.card_type = data.sub_cards.join("+");
    } else if let Some(first) = data.sub_cards.first() {
        data.card_type = first.clone();
    }
    Ok(())
}

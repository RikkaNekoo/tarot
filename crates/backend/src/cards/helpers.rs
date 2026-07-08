//! 卡种读取的共享子流程，对应 nfsee `read.js` 中被多张卡复用的函数。
//!
//! 这些函数只负责抓取原始字节写入 [`RawCardData`]，不做语义解析。

use tarot_core::{Apdu, RawCardData, Result, Transceiver};

/// SELECT by AID（`00 A4 04 00 Lc <AID> 00`）。
/// 返回是否成功（SW=9000），成功时把 FCI 原始数据写入 `key`。
pub fn select_aid<T: Transceiver>(
    tx: &mut T,
    aid: &[u8],
    data: &mut RawCardData,
    key: &str,
) -> Result<bool> {
    let apdu = Apdu::case4(0x00, 0xA4, 0x04, 0x00, aid, 0x00);
    let resp = tx.transceive(&apdu)?;
    if resp.is_ok() {
        data.put(key, resp.data_hex());
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 读取 PBOC 交通卡通用信息：余额(805C)、交易记录(00B2..C4)、圈存 ATC(8050)。
///
/// 对应 nfsee 的 `ReadPBOCBalanceATCAndTrans(usage)`。
/// `usage` 默认为 2（电子钱包）。抓到的原始字节以带序号的键写入 `data`：
/// - `balance`：余额查询响应数据
/// - `trans_N`：第 N 条交易记录（原始 23 字节）
/// - `load_atc`：圈存初始化响应
pub fn read_pboc_balance_atc_trans<T: Transceiver>(
    tx: &mut T,
    usage: u8,
    data: &mut RawCardData,
    prefix: &str,
) -> Result<()> {
    // 余额：805C 00 <usage> 04
    let bal = Apdu::from_bytes(vec![0x80, 0x5C, 0x00, usage, 0x04]);
    let r = tx.transceive(&bal)?;
    if r.is_ok() {
        data.put(format!("{prefix}balance"), r.data_hex());
    }

    // 交易记录：00 B2 <n> C4 17，循环到失败为止
    for n in 1u8..=10 {
        let rec = Apdu::from_bytes(vec![0x00, 0xB2, n, 0xC4, 0x17]);
        let r = tx.transceive(&rec)?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("{prefix}trans_{n}"), r.data_hex());
    }

    // 圈存 ATC：8050 00 <usage> 0B <11 bytes>
    let init_data = [
        0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
    ];
    let load = Apdu::case3(0x80, 0x50, 0x00, usage, &init_data);
    let r = tx.transceive(&load)?;
    if r.is_ok() {
        data.put(format!("{prefix}load_atc"), r.data_hex());
    }
    Ok(())
}

/// 读取基础信息文件（15 号文件）：`00 B0 95 00 1E`。
/// 对应 nfsee 的 `BasicInfoFile`（这里不解析 FCI TLV，直接读文件）。
pub fn read_basic_info_file<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    key: &str,
) -> Result<bool> {
    let apdu = Apdu::from_bytes(vec![0x00, 0xB0, 0x95, 0x00, 0x1E]);
    let r = tx.transceive(&apdu)?;
    if r.is_ok() {
        data.put(key, r.data_hex());
        Ok(true)
    } else {
        Ok(false)
    }
}

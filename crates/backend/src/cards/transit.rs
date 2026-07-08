//! PBOC 交通卡与校园卡读取流程，对应 nfsee 各 `ReadTransXXX` 函数。
//! 只抓原始字节，字段偏移解析交给前端。

use super::aids;
use super::helpers::{read_basic_info_file, read_pboc_balance_atc_trans, select_aid};
use tarot_core::{Apdu, RawCardData, Result, Transceiver};

/// 已 SELECT 成功后，按卡名读取该卡的余额/交易/信息文件。
pub fn read_by_name<T: Transceiver>(tx: &mut T, name: &str, data: &mut RawCardData) -> Result<()> {
    match name {
        "ShenzhenTong" | "WuhanTong" | "CityUnion" | "TUnion" => {
            read_basic_info_file(tx, data, &format!("{name}_file15"))?;
            read_pboc_balance_atc_trans(tx, 2, data, &format!("{name}_"))?;
            // TUnion 额外读 DF11（17 号文件）与 SFI 0x1E 行程记录
            if name == "TUnion" {
                let apdu = Apdu::from_bytes(vec![0x00, 0xB0, 0x97, 0x00, 0x0B]);
                let r = tx.transceive(&apdu)?;
                if r.is_ok() {
                    data.put("TUnion_file17", r.data_hex());
                }
                // SFI 0x1E 行程记录：00 B2 <n> F4 00（P2 = 0x1E<<3 | 4）
                for n in 1u8..=10 {
                    let rec = Apdu::from_bytes(vec![0x00, 0xB2, n, 0xF4, 0x00]);
                    let r = tx.transceive(&rec)?;
                    if !r.is_ok() {
                        break;
                    }
                    data.put(format!("TUnion_trip_{n}"), r.data_hex());
                }
            }
            // CityUnion 重庆特例：切到 MF 读 85 文件
            if name == "CityUnion" {
                probe_chongqing(tx, data)?;
            }
        }
        "LingnanPass" => {
            read_basic_info_file(tx, data, "LingnanPass_file15")?;
            // 二次 SELECT PAY.TICL
            select_aid(tx, aids::LINGNAN_TICL, data, "LingnanPass_ticl")?;
            read_pboc_balance_atc_trans(tx, 2, data, "LingnanPass_")?;
        }
        "MacauPass" => {
            read_basic_info_file(tx, data, "MacauPass_file15")?;
            read_pboc_balance_atc_trans(tx, 2, data, "MacauPass_")?;
        }
        "TMoney" => read_tmoney(tx, data)?,
        _ => {}
    }
    Ok(())
}

/// 北京一卡通（BMAC）：直接读 0x84 文件（无需 SELECT），命中则读余额/交易。
/// 对应 nfsee `ReadTransBeijing`。
pub fn probe_beijing<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<bool> {
    let apdu = Apdu::from_bytes(vec![0x00, 0xB0, 0x84, 0x00, 0x20]);
    let r = tx.transceive(&apdu)?;
    // 命中特征：SW=9000 且内容以 1000 开头
    if r.is_ok() && r.data.first() == Some(&0x10) && r.data.get(1) == Some(&0x00) {
        data.put("BMAC_file04", r.data_hex());
        // 北京卡需要 SELECT 1001 目录后再读钱包
        select_aid_short(tx, &[0x10, 0x01], data, "BMAC_df")?;
        read_pboc_balance_atc_trans(tx, 2, data, "BMAC_")?;
        data.sub_cards.push("BMAC".to_string());
        return Ok(true);
    }
    Ok(false)
}

/// SELECT by short DF ID（`00 A4 00 00 02 <id>`）。
fn select_aid_short<T: Transceiver>(
    tx: &mut T,
    id: &[u8],
    data: &mut RawCardData,
    key: &str,
) -> Result<bool> {
    let apdu = Apdu::case3(0x00, 0xA4, 0x00, 0x00, id);
    let r = tx.transceive(&apdu)?;
    if r.is_ok() {
        data.put(key, r.data_hex());
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 重庆 City Union 特例：SELECT MF(3F00) 再读 85 文件（48 字节）。
fn probe_chongqing<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    let mf = Apdu::from_bytes(vec![0x00, 0xA4, 0x00, 0x00, 0x02, 0x3F, 0x00]);
    if tx.transceive(&mf)?.is_ok() {
        let f = Apdu::from_bytes(vec![0x00, 0xB0, 0x85, 0x00, 0x30]);
        let r = tx.transceive(&f)?;
        if r.is_ok() {
            data.put("CityUnion_chongqing_file", r.data_hex());
        }
    }
    Ok(())
}

/// T-Money（韩国）：读余额(904C) 与余额记录(00B2..24)。
/// 对应 nfsee `ReadTMoney`。FCI 中已含 purse info，前端解析。
fn read_tmoney<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    let bal = Apdu::from_bytes(vec![0x90, 0x4C, 0x00, 0x00, 0x04]);
    let r = tx.transceive(&bal)?;
    if r.is_ok() {
        data.put("TMoney_balance", r.data_hex());
    }
    for n in 1u8..=10 {
        let rec = Apdu::from_bytes(vec![0x00, 0xB2, n, 0x24, 0x2E]);
        let r = tx.transceive(&rec)?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("TMoney_trans_{n}"), r.data_hex());
    }
    Ok(())
}

/// Macau Pass 备用路径：SELECT AMTJAVACARD 容器后再选 Macau AID。
/// 对应 nfsee 中在探测链末尾的兜底逻辑。
pub fn probe_macau_fallback<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    // 若主路径已命中 Macau，则跳过
    if data.sub_cards.iter().any(|c| c == "MacauPass") {
        return Ok(());
    }
    if select_aid(tx, aids::AMTJAVACARD, data, "amtjavacard_fci")? {
        let macau_aid = &aids::TYPE_A_CHAIN[5].aid; // MacauPass
        if select_aid(tx, macau_aid, data, "macau_fci")? {
            read_basic_info_file(tx, data, "MacauPass_file15")?;
            read_pboc_balance_atc_trans(tx, 2, data, "MacauPass_")?;
            data.sub_cards.push("MacauPass".to_string());
        }
    }
    Ok(())
}

//! PBOC 交通卡与校园卡读取流程，对照交通卡 APDU 分析报告的公开读取路径。
//! 只抓原始字节，字段偏移解析交给前端。

use super::aids;
use super::helpers::select_aid;
use tarot_core::{Apdu, ApduResponse, RawCardData, Result, Transceiver};

const MAX_TRANSIT_RECORDS: u8 = 31;
const STANDARD_TRANSIT_NAMES: &[&str] = &[
    "ShenzhenTong",
    "WuhanTong",
    "CityUnion",
    "TUnion",
    "MotBmac",
    "ChinaTransit",
    "SuXin",
    "SzpkZyy",
];

/// 已 SELECT 成功后，按卡名读取该卡的余额/交易/信息文件。
pub fn read_by_name<T: Transceiver>(tx: &mut T, name: &str, data: &mut RawCardData) -> Result<()> {
    match name {
        name if STANDARD_TRANSIT_NAMES.contains(&name) => {
            read_standard_transit(tx, data, name)?;
            // TUnion 额外读 DF11（17 号文件）与 SFI 0x1E 行程记录
            if name == "TUnion" {
                let apdu = Apdu::from_bytes(vec![0x00, 0xB0, 0x97, 0x00, 0x0B]);
                let r = transceive_retry(tx, apdu)?;
                if r.is_ok() {
                    data.put("TUnion_file17", r.data_hex());
                }
                // SFI 0x1E 行程记录：00 B2 <n> F4 00（P2 = 0x1E<<3 | 4）
                for n in 1u8..=10 {
                    let rec = Apdu::from_bytes(vec![0x00, 0xB2, n, 0xF4, 0x00]);
                    let r = transceive_retry(tx, rec)?;
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
            read_fixed_files(tx, data, "LingnanPass")?;
            // 二次 SELECT PAY.TICL
            select_aid(tx, aids::LINGNAN_TICL, data, "LingnanPass_ticl")?;
            read_transit_balance(tx, data, "LingnanPass_")?;
            read_transit_records(tx, data, "LingnanPass_")?;
            read_month_ticket_record(tx, data, "LingnanPass")?;
            read_lingnan_guangzhou(tx, data)?;
        }
        "MacauPass" => {
            read_standard_transit(tx, data, "MacauPass")?;
        }
        "TMoney" => read_tmoney(tx, data)?,
        _ => {}
    }
    Ok(())
}

/// 交通卡 APDU 层重试：对 `6Cxx` 和 `6700` 修正 Le 后重发。
fn transceive_retry<T: Transceiver>(tx: &mut T, apdu: Apdu) -> Result<ApduResponse> {
    let resp = tx.transceive(&apdu)?;
    let mut bytes = apdu.as_bytes().to_vec();
    if bytes.is_empty() {
        return Ok(resp);
    }
    if resp.sw1 == 0x6C {
        bytes.pop();
        bytes.push(resp.sw2);
        return tx.transceive(&Apdu::from_bytes(bytes));
    }
    if resp.sw() == 0x6700 && bytes.last() != Some(&0x00) {
        bytes.pop();
        bytes.push(0x00);
        return tx.transceive(&Apdu::from_bytes(bytes));
    }
    Ok(resp)
}

fn put_if_ok<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    key: impl Into<String>,
    bytes: Vec<u8>,
) -> Result<bool> {
    let r = transceive_retry(tx, Apdu::from_bytes(bytes))?;
    if r.is_ok() {
        data.put(key, r.data_hex());
        Ok(true)
    } else {
        Ok(false)
    }
}

fn read_standard_transit<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    name: &str,
) -> Result<()> {
    read_fixed_files(tx, data, name)?;
    read_transit_balance(tx, data, &format!("{name}_"))?;
    read_transit_records(tx, data, &format!("{name}_"))?;
    read_month_ticket_record(tx, data, name)?;
    Ok(())
}

fn read_fixed_files<T: Transceiver>(tx: &mut T, data: &mut RawCardData, name: &str) -> Result<()> {
    put_if_ok(
        tx,
        data,
        format!("{name}_file04"),
        vec![0x00, 0xB0, 0x84, 0x00, 0x3C],
    )?;
    put_if_ok(
        tx,
        data,
        format!("{name}_file05"),
        vec![0x00, 0xB0, 0x85, 0x00, 0x20],
    )?;
    if put_if_ok(
        tx,
        data,
        format!("{name}_file15"),
        vec![0x00, 0xB0, 0x95, 0x00, 0x46],
    )? {
        return Ok(());
    }
    put_if_ok(
        tx,
        data,
        format!("{name}_file15"),
        vec![0x00, 0xB0, 0x95, 0x00, 0x1E],
    )?;
    Ok(())
}

fn read_transit_balance<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    prefix: &str,
) -> Result<()> {
    let balance_commands = [
        ("balance", vec![0x80, 0x5C, 0x00, 0x02, 0x04]),
        ("balance_ext", vec![0x80, 0x5C, 0x05, 0x02, 0x10]),
        ("balance_usage1", vec![0x80, 0x5C, 0x00, 0x01, 0x04]),
    ];
    for (key, bytes) in balance_commands {
        put_if_ok(tx, data, format!("{prefix}{key}"), bytes)?;
    }

    let init_data = [
        0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
    ];
    let r = transceive_retry(tx, Apdu::case3(0x80, 0x50, 0x00, 0x02, &init_data))?;
    if r.is_ok() {
        data.put(format!("{prefix}load_atc"), r.data_hex());
    }
    Ok(())
}

fn read_transit_records<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    prefix: &str,
) -> Result<()> {
    // 兼容原始 key：先读常见 SFI 0x18/P2 C4 的 0x17 字节交易记录。
    for n in 1u8..=MAX_TRANSIT_RECORDS {
        let r = transceive_retry(tx, Apdu::from_bytes(vec![0x00, 0xB2, n, 0xC4, 0x17]))?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("{prefix}trans_{n}"), r.data_hex());
    }

    for n in 1u8..=MAX_TRANSIT_RECORDS {
        let r = transceive_retry(tx, Apdu::from_bytes(vec![0x00, 0xB2, n, 0x84, 0x17]))?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("{prefix}record84_{n}"), r.data_hex());
    }

    for n in 1u8..=MAX_TRANSIT_RECORDS {
        let r = transceive_retry(tx, Apdu::from_bytes(vec![0x00, 0xB2, n, 0x3C, 0x10]))?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("{prefix}record3c_{n}"), r.data_hex());
    }

    for n in 1u8..=MAX_TRANSIT_RECORDS {
        let r = transceive_retry(tx, Apdu::from_bytes(vec![0x00, 0xB2, n, 0x9C, 0x17]))?;
        if !r.is_ok() {
            break;
        }
        data.put(format!("{prefix}record9c_{n}"), r.data_hex());
    }
    Ok(())
}

fn read_month_ticket_record<T: Transceiver>(
    tx: &mut T,
    data: &mut RawCardData,
    name: &str,
) -> Result<()> {
    put_if_ok(
        tx,
        data,
        format!("{name}_record19_6"),
        vec![0x00, 0xB2, 0x06, 0xC8, 0x30],
    )?;
    Ok(())
}

fn read_lingnan_guangzhou<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    put_if_ok(
        tx,
        data,
        "LingnanPass_guangzhou_monthly",
        vec![0x00, 0xB2, 0x01, 0x44, 0x16],
    )?;
    Ok(())
}

/// 北京一卡通（BMAC）：直接读 0x84 文件（无需 SELECT），命中则读余额/交易。
/// 对应 nfsee `ReadTransBeijing`。
pub fn probe_beijing<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<bool> {
    let apdu = Apdu::from_bytes(vec![0x00, 0xB0, 0x84, 0x00, 0x20]);
    let r = transceive_retry(tx, apdu)?;
    // 命中特征：SW=9000 且内容以 1000 开头
    if r.is_ok() && r.data.first() == Some(&0x10) && r.data.get(1) == Some(&0x00) {
        data.put("BMAC_file04", r.data_hex());
        // 北京卡需要 SELECT 1001 目录后再读钱包
        select_aid_short(tx, &[0x10, 0x01], data, "BMAC_df")?;
        read_transit_balance(tx, data, "BMAC_")?;
        read_transit_records(tx, data, "BMAC_")?;
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
    let r = transceive_retry(tx, apdu)?;
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
    if transceive_retry(tx, mf)?.is_ok() {
        let f = Apdu::from_bytes(vec![0x00, 0xB0, 0x85, 0x00, 0x30]);
        let r = transceive_retry(tx, f)?;
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
    let r = transceive_retry(tx, bal)?;
    if r.is_ok() {
        data.put("TMoney_balance", r.data_hex());
    }
    for n in 1u8..=10 {
        let rec = Apdu::from_bytes(vec![0x00, 0xB2, n, 0x24, 0x2E]);
        let r = transceive_retry(tx, rec)?;
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
        let Some(macau_aid) = aids::TYPE_A_CHAIN
            .iter()
            .find(|entry| entry.name == "MacauPass")
            .map(|entry| entry.aid)
        else {
            return Ok(());
        };
        if select_aid(tx, macau_aid, data, "macau_fci")? {
            read_standard_transit(tx, data, "MacauPass")?;
            data.sub_cards.push("MacauPass".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarot_core::{Apdu, ApduResponse, Result, Transceiver};

    #[derive(Default)]
    struct MockTx {
        seen: Vec<String>,
        responses: Vec<ApduResponse>,
    }

    impl MockTx {
        fn new(responses: Vec<ApduResponse>) -> Self {
            Self {
                seen: Vec::new(),
                responses,
            }
        }
    }

    impl Transceiver for MockTx {
        fn transceive(&mut self, apdu: &Apdu) -> Result<ApduResponse> {
            self.seen.push(apdu.to_hex());
            Ok(self.responses.remove(0))
        }
    }

    fn response(data: &[u8], sw1: u8, sw2: u8) -> ApduResponse {
        ApduResponse {
            data: data.to_vec(),
            sw1,
            sw2,
        }
    }

    #[test]
    fn retry_6c_uses_suggested_le() {
        let mut tx = MockTx::new(vec![
            response(&[], 0x6C, 0x20),
            response(&[0x12], 0x90, 0x00),
        ]);

        let r = transceive_retry(
            &mut tx,
            Apdu::from_bytes(vec![0x00, 0xB0, 0x84, 0x00, 0x3C]),
        )
        .unwrap();

        assert!(r.is_ok());
        assert_eq!(tx.seen, vec!["00B084003C", "00B0840020"]);
    }

    #[test]
    fn retry_6700_uses_zero_le() {
        let mut tx = MockTx::new(vec![
            response(&[], 0x67, 0x00),
            response(&[0x34], 0x90, 0x00),
        ]);

        let r = transceive_retry(
            &mut tx,
            Apdu::from_bytes(vec![0x80, 0x5C, 0x05, 0x02, 0x10]),
        )
        .unwrap();

        assert!(r.is_ok());
        assert_eq!(tx.seen, vec!["805C050210", "805C050200"]);
    }
}

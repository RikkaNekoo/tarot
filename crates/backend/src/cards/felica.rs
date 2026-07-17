//! FeliCa（Octopus/八达通）读取，对应 nfsee `ReadOctopus` + `felica.js`。
//!
//! 支持两种 FeliCa 访问方式：
//! - ACR1251U FeliCa 访问方式（见 API 手册 5.2.6）：
//! - 取 IDm：标准 `FF CA 00 00 00`，返回 8 字节 IDm + 9000。
//! - 访问 FeliCa 命令：`FF 00 00 00 <Lc> <FeliCa命令>`，其中 FeliCa 命令
//!   以长度字节开头：`<len> <cmd> <payload...>`（len 含自身）。
//! - Sony PaSoRi RC-S300：`FF FE` Data Exchange DIRECT 通信，命令和响应均为
//!   从长度字节开始的完整 FeliCa frame。
//!
//! 手册读内存块示例：
//! `FF 00 00 00 10  10 06 <IDm(8)> 01 <svcLo> <svcHi> 01 80 <addr>`
//! - `10` = Lc（FeliCa 命令 16 字节）
//! - `10` = FeliCa 命令长度
//! - `06` = Read Without Encryption
//! - IDm(8) + 服务数(01) + 服务码(小端2) + 块数(01) + 块元素(80 addr)
//!
//! 八达通参数（来自 nfsee felica.js）：系统码 0x0880，余额服务码 0x0117。

use tarot_core::{Apdu, Error, RawCardData, Result, Transceiver};

/// 八达通余额服务码（小端发送）。
const BALANCE_SERVICE: u16 = 0x0117;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FelicaTransport {
    AcrDirect,
    RcS300DataExchange,
}

/// 读取 FeliCa/八达通：取 IDm 识别，再用 Read Without Encryption 读余额块。
pub fn read_octopus<T: Transceiver>(
    tx: &mut T,
    reader_name: &str,
    data: &mut RawCardData,
) -> Result<()> {
    data.card_type = "Felica".into();

    // 1. 取 IDm：FF CA 00 00 00
    let get_idm = Apdu::from_bytes(vec![0xFF, 0xCA, 0x00, 0x00, 0x00]);
    let r = tx.transceive(&get_idm)?;
    if !r.is_ok() || r.data.len() < 8 {
        return Ok(());
    }
    let idm = r.data[..8].to_vec();
    data.put("felica_idm", hex::encode_upper(&idm));
    data.card_type = "Octopus".into();
    data.sub_cards.push("Octopus".to_string());

    // 2. Read Without Encryption 读余额块（block 0）
    match transport_for_reader(reader_name) {
        FelicaTransport::RcS300DataExchange => {
            data.put("felica_transport", "rc_s300_data_exchange");
            read_block_rc_s300(
                tx,
                &idm,
                BALANCE_SERVICE,
                0x00,
                data,
                "felica_balance_block",
            );
        }
        FelicaTransport::AcrDirect => {
            data.put("felica_transport", "acr_direct");
            read_block_acr_direct(
                tx,
                &idm,
                BALANCE_SERVICE,
                0x00,
                data,
                "felica_balance_block",
            );
        }
    }

    Ok(())
}

fn transport_for_reader(reader_name: &str) -> FelicaTransport {
    let name = reader_name.to_ascii_lowercase();
    if name.contains("rc-s300") || name.contains("felica port/pasori 4.0") {
        FelicaTransport::RcS300DataExchange
    } else {
        FelicaTransport::AcrDirect
    }
}

/// 用 ACR FeliCa 直传命令读一个内存块。
///
/// 构造 FeliCa 命令：`<len> 06 <IDm(8)> 01 <svcLo> <svcHi> 01 80 <addr>`，
/// 再封装为 `FF 00 00 00 <Lc> <FeliCa命令>`。
fn read_block_acr_direct<T: Transceiver>(
    tx: &mut T,
    idm: &[u8],
    service: u16,
    addr: u8,
    data: &mut RawCardData,
    key: &str,
) {
    // 构造 FeliCa 命令体（不含开头 len）。
    let mut felica_cmd = Vec::new();
    felica_cmd.push(0x06); // Read Without Encryption
    felica_cmd.extend_from_slice(idm); // IDm 8 字节
    felica_cmd.push(0x01); // 服务数量
    felica_cmd.push((service & 0xff) as u8); // 服务码低字节
    felica_cmd.push((service >> 8) as u8); // 服务码高字节
    felica_cmd.push(0x01); // 块数量
    felica_cmd.push(0x80); // 块列表元素（2字节模式）
    felica_cmd.push(addr); // 块地址

    // 在最前面加长度字节（含自身）。
    let mut frame = Vec::with_capacity(felica_cmd.len() + 1);
    frame.push((felica_cmd.len() + 1) as u8);
    frame.extend_from_slice(&felica_cmd);

    // 封装为 FF 00 00 00 <Lc> <frame>
    let apdu = Apdu::case3(0xFF, 0x00, 0x00, 0x00, &frame);
    if let Ok(r) = tx.transceive(&apdu) {
        data.put(key, r.to_hex());
    }
}

/// 用 RC-S300 `FF FE` Data Exchange DIRECT 通信读一个 FeliCa 内存块。
fn read_block_rc_s300<T: Transceiver>(
    tx: &mut T,
    idm: &[u8],
    service: u16,
    addr: u8,
    data: &mut RawCardData,
    key: &str,
) {
    let frame = build_read_without_encryption(idm, service, addr);
    let apdu = build_rc_s300_data_exchange(&frame);
    if let Ok(response) = tx.transceive(&apdu) {
        if response.is_ok() && validate_felica_read_response(&response.data).is_ok() {
            data.put(key, hex::encode_upper(response.data));
        }
    }
}

fn build_rc_s300_data_exchange(frame: &[u8]) -> Apdu {
    Apdu::case4(0xFF, 0xFE, 0x01, 0x00, frame, 0x00)
}

fn build_read_without_encryption(idm: &[u8], service: u16, addr: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(16);
    frame.push(0x10); // FeliCa frame length, including this byte.
    frame.push(0x06); // Read Without Encryption
    frame.extend_from_slice(idm);
    frame.push(0x01); // service count
    frame.push((service & 0xff) as u8);
    frame.push((service >> 8) as u8);
    frame.push(0x01); // block count
    frame.push(0x80); // two-byte block list element
    frame.push(addr);
    frame
}

fn validate_felica_read_response(frame: &[u8]) -> Result<()> {
    if frame.len() < 13 || frame[1] != 0x07 {
        return Err(Error::Other("unexpected FeliCa read response".into()));
    }
    if frame[10] != 0x00 || frame[11] != 0x00 {
        return Err(Error::Other(format!(
            "FeliCa status flags {:02X}{:02X}",
            frame[10], frame[11]
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_data_exchange_for_rc_s300_names() {
        assert_eq!(
            transport_for_reader("Sony FeliCa Port/PaSoRi 4.0 00 00"),
            FelicaTransport::RcS300DataExchange
        );
        assert_eq!(
            transport_for_reader("RC-S300/P"),
            FelicaTransport::RcS300DataExchange
        );
    }

    #[test]
    fn does_not_match_other_sony_pasori_readers() {
        assert_eq!(
            transport_for_reader("SONY PaSoRi RC-S380"),
            FelicaTransport::AcrDirect
        );
        assert_eq!(
            transport_for_reader("SONY FeliCa Reader"),
            FelicaTransport::AcrDirect
        );
    }

    #[test]
    fn keeps_acr_direct_for_acs_names() {
        assert_eq!(
            transport_for_reader("ACS ACR1251U PICC Interface"),
            FelicaTransport::AcrDirect
        );
    }

    #[test]
    fn builds_octopus_read_frame() {
        let frame = build_read_without_encryption(&[1, 2, 3, 4, 5, 6, 7, 8], 0x0117, 0);
        assert_eq!(hex::encode_upper(frame), "10060102030405060708011701018000");
    }

    #[test]
    fn wraps_rc_s300_frame_in_direct_data_exchange() {
        let frame = build_read_without_encryption(&[1, 2, 3, 4, 5, 6, 7, 8], 0x0117, 0);
        assert_eq!(
            build_rc_s300_data_exchange(&frame).to_hex(),
            "FFFE0100101006010203040506070801170101800000"
        );
    }
}

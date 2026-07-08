//! FeliCa（Octopus/八达通）读取，对应 nfsee `ReadOctopus` + `felica.js`。
//!
//! ACR1251U FeliCa 访问方式（见 API 手册 5.2.6）：
//! - 取 IDm：标准 `FF CA 00 00 00`，返回 8 字节 IDm + 9000。
//! - 访问 FeliCa 命令：`FF 00 00 00 <Lc> <FeliCa命令>`，其中 FeliCa 命令
//!   以长度字节开头：`<len> <cmd> <payload...>`（len 含自身）。
//!
//! 手册读内存块示例：
//! `FF 00 00 00 10  10 06 <IDm(8)> 01 <svcLo> <svcHi> 01 80 <addr>`
//! - `10` = Lc（FeliCa 命令 16 字节）
//! - `10` = FeliCa 命令长度
//! - `06` = Read Without Encryption
//! - IDm(8) + 服务数(01) + 服务码(小端2) + 块数(01) + 块元素(80 addr)
//!
//! 八达通参数（来自 nfsee felica.js）：系统码 0x0880，余额服务码 0x0117。

use tarot_core::{Apdu, RawCardData, Result, Transceiver};

/// 八达通余额服务码（小端发送）。
const BALANCE_SERVICE: u16 = 0x0117;

/// 读取 FeliCa/八达通：取 IDm 识别，再用 Read Without Encryption 读余额块。
pub fn read_octopus<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
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
    read_block(
        tx,
        &idm,
        BALANCE_SERVICE,
        0x00,
        data,
        "felica_balance_block",
    );

    Ok(())
}

/// 用 ACR FeliCa 直传命令读一个内存块。
///
/// 构造 FeliCa 命令：`<len> 06 <IDm(8)> 01 <svcLo> <svcHi> 01 80 <addr>`，
/// 再封装为 `FF 00 00 00 <Lc> <FeliCa命令>`。
fn read_block<T: Transceiver>(
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

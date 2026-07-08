//! Mifare 系列读取，对应 nfsee `ReadMifareUltralight/Classic/DESFire`。
//!
//! PC/SC 移植要点：与手机 NFC 直接下裸命令不同，ACR 读卡器（ACR1251U）通过
//! PC/SC 伪 APDU 发送 Mifare 命令：
//! - READ BINARY BLOCK：`FF B0 00 <block> <len>`
//! - Ultralight READ（4 页）：同样用 `FF B0 00 <page> 10`
//! - LOAD KEY：`FF 82 ...`，AUTHENTICATE：`FF 86 ...`（Classic）
//! DESFire 走 ISO7816 包装命令（`90 xx ...`）。

use tarot_core::{Apdu, RawCardData, Result, Transceiver};

/// Mifare Ultralight / NTAG：逐页读取（每次 4 页 = 16 字节）。
pub fn read_ultralight<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    data.card_type = "MifareUltralight".into();
    // 最多读 ~60 页（覆盖 NTAG216 231 页需调大；这里读前 60 页作框架示例）。
    let mut all = String::new();
    for page in (0u8..60).step_by(4) {
        // FF B0 00 <page> 10：读 16 字节（4 页）
        let apdu = Apdu::from_bytes(vec![0xFF, 0xB0, 0x00, page, 0x10]);
        let r = tx.transceive(&apdu)?;
        if !r.is_ok() {
            break;
        }
        all.push_str(&r.data_hex());
    }
    if !all.is_empty() {
        data.put("ul_data", all);
    }
    Ok(())
}

/// Mifare Classic：用公共默认密钥认证并读取前 3 个扇区（MAD/NDEF 区）。
/// 对应 nfsee 中用 A0A1.. 与 D3F7.. 尝试认证。
pub fn read_classic<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<()> {
    data.card_type = "MifareClassic".into();
    // 公共默认密钥（KEY A）
    let keys: [[u8; 6]; 3] = [
        [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5],
        [0xD3, 0xF7, 0xD3, 0xF7, 0xD3, 0xF7],
        [0xD3, 0xF7, 0xD3, 0xF7, 0xD3, 0xF7],
    ];
    let mut all = String::new();
    for (sector, key) in keys.iter().enumerate() {
        let begin_block = (sector * 4) as u8;
        // LOAD KEY 到 volatile slot 0：FF 82 00 00 06 <key>
        let load = Apdu::case3(0xFF, 0x82, 0x00, 0x00, key);
        if !tx.transceive(&load)?.is_ok() {
            break;
        }
        // AUTHENTICATE block：FF 86 00 00 05 01 00 <block> 60 00 (KEY A, slot 0)
        let auth_data = [0x01, 0x00, begin_block, 0x60, 0x00];
        let auth = Apdu::case3(0xFF, 0x86, 0x00, 0x00, &auth_data);
        if !tx.transceive(&auth)?.is_ok() {
            break;
        }
        // 读 4 个块
        for b in 0..4u8 {
            let read = Apdu::from_bytes(vec![0xFF, 0xB0, 0x00, begin_block + b, 0x10]);
            let r = tx.transceive(&read)?;
            if r.is_ok() {
                all.push_str(&r.data_hex());
            }
        }
    }
    if !all.is_empty() {
        data.put("classic_data", all);
    }
    Ok(())
}

/// 探测 DESFire：GetVersion（`90 60 00 00 00`），命中 SW=91AF 则记录版本。
/// 对应 nfsee 的 DESFire 分支。
pub fn probe_desfire<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<bool> {
    let apdu = Apdu::from_bytes(vec![0x90, 0x60, 0x00, 0x00, 0x00]);
    let r = tx.transceive(&apdu)?;
    // DESFire 成功且要求更多数据：SW=91AF
    if r.sw() == 0x91AF {
        data.put("desfire_version_1", r.data_hex());
        // 后续 91AF 分片
        for i in 2..=3u8 {
            let more = Apdu::from_bytes(vec![0x90, 0xAF, 0x00, 0x00, 0x00]);
            let rr = tx.transceive(&more)?;
            data.put(format!("desfire_version_{i}"), rr.data_hex());
            if rr.sw() != 0x91AF {
                break;
            }
        }
        data.card_type = "MifareDESFire".into();
        data.sub_cards.push("MifareDESFire".to_string());
        return Ok(true);
    }
    Ok(false)
}

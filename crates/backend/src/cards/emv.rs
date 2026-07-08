//! EMV 非接读取流程，对应 nfsee `ReadPPSE`。
//!
//! 后端职责：完成 SELECT PPSE → SELECT AID → GPO → 读 AFL 记录 → 取 ATC/PIN retry/日志，
//! 全部以原始字节写入 `RawCardData`。PDOL 构造与 TLV 解析交给前端；此处用最小
//! PDOL（长度 0）发起 GPO，若卡要求 PDOL 前端可二次交互（当前实现覆盖多数无 PDOL 卡）。

use super::aids;
use super::helpers::select_aid;
use tarot_core::{Apdu, RawCardData, Result, Transceiver};

/// 探测并读取 EMV 应用。返回是否命中 PPSE。
pub fn probe_ppse<T: Transceiver>(tx: &mut T, data: &mut RawCardData) -> Result<bool> {
    if !select_aid(tx, aids::PPSE, data, "ppse_fci")? {
        return Ok(false);
    }

    // 从 PPSE FCI 中解析候选 AID（tag 4F），逐个尝试 SELECT。
    // 这是识别 EMV 应用所需的最小解析，避免维护脆弱的完整 AID 硬编码表。
    let ppse_hex = data.get("ppse_fci").unwrap_or_default().to_string();
    let candidate_aids = extract_aids_from_ppse(&ppse_hex);

    // 若候选 AID 全部属于交通联合（A0000006320101..，在 PPSE 注册的非银行应用），
    // 则不算独立 EMV 卡，避免与交通联合叠加时误报。
    let all_transit = !candidate_aids.is_empty()
        && candidate_aids.iter().all(|a| a.starts_with("A000000632"));
    if all_transit {
        return Ok(false);
    }

    let mut selected = false;
    for aid_hex in &candidate_aids {
        let aid = hex::decode(aid_hex).unwrap_or_default();
        if aid.is_empty() {
            continue;
        }
        if select_aid(tx, &aid, data, "emv_app_fci")? {
            let brand = brand_of_aid(aid_hex);
            data.card_type = format!("EMV:{brand}");
            data.put("emv_aid", aid_hex);
            data.put("emv_brand", brand);
            selected = true;
            break;
        }
    }
    if !selected {
        // PPSE 命中但候选 AID 都 SELECT 失败：仍标记为 EMV（已抓 ppse_fci 供前端解析）。
        data.card_type = "EMV".into();
        return Ok(true);
    }

    // GPO：最小 PDOL（Lc=02, 数据 83 00，Le=00）
    let gpo = Apdu::from_bytes(vec![0x80, 0xA8, 0x00, 0x00, 0x02, 0x83, 0x00, 0x00]);
    let r = tx.transceive(&gpo)?;
    if r.is_ok() {
        data.put("emv_gpo", r.data_hex());
    }

    // 读常见 SFI/记录（1..3 号 SFI，1..4 号记录），抓原始字节供前端解析 AFL/Track2。
    for sfi in 1u8..=3 {
        for rec in 1u8..=4 {
            let p2 = (sfi << 3) | 0x04;
            let apdu = Apdu::from_bytes(vec![0x00, 0xB2, rec, p2, 0x00]);
            let rr = tx.transceive(&apdu)?;
            if rr.is_ok() {
                data.put(format!("emv_rec_{sfi}_{rec}"), rr.data_hex());
            }
        }
    }

    // ATC：80CA9F3600
    read_get_data(tx, &[0x9F, 0x36], data, "emv_atc")?;
    // PIN retry：80CA9F1700
    read_get_data(tx, &[0x9F, 0x17], data, "emv_pin_retry")?;
    // 交易日志格式：80CA9F4F00
    read_get_data(tx, &[0x9F, 0x4F], data, "emv_log_format")?;

    Ok(true)
}

/// GET DATA（`80 CA <tag> 00`），抓取原始响应。
fn read_get_data<T: Transceiver>(
    tx: &mut T,
    tag: &[u8; 2],
    data: &mut RawCardData,
    key: &str,
) -> Result<()> {
    let apdu = Apdu::from_bytes(vec![0x80, 0xCA, tag[0], tag[1], 0x00]);
    let r = tx.transceive(&apdu)?;
    if r.is_ok() {
        data.put(key, r.data_hex());
    }
    Ok(())
}

/// 从 PPSE FCI 的十六进制中提取所有 tag `4F`（ADF Name / AID）的值。
///
/// 只做定位 `4F` 标签这一层最小 TLV 扫描：读到 `4F` 后下一字节为长度，
/// 随后 length 字节即为 AID。返回大写十六进制 AID 列表（按出现顺序）。
fn extract_aids_from_ppse(ppse_hex: &str) -> Vec<String> {
    let bytes = match hex::decode(ppse_hex) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let mut aids = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0x4F {
            let len = bytes[i + 1] as usize;
            let start = i + 2;
            if start + len <= bytes.len() && (5..=16).contains(&len) {
                aids.push(hex::encode_upper(&bytes[start..start + len]));
                i = start + len;
                continue;
            }
        }
        i += 1;
    }
    aids
}

/// 按 AID 前缀映射品牌名（RID 部分）。未知返回 "Unknown"。
fn brand_of_aid(aid_hex: &str) -> String {
    for (prefix, name) in aids::EMV_AID_NAMES {
        if aid_hex.starts_with(prefix) {
            return name.to_string();
        }
    }
    "Unknown".to_string()
}

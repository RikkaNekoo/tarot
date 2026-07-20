//! 八达通（Octopus，FeliCa）解析。
//! IDm 来自 `felica_idm`；余额来自 `felica_balance_block`：
//! ACR 直传响应结构去掉尾部 SW 后，块数据前 4 字节大端为余额，
//! `(值 - 500) * 10` 得单位 $0.01 港币。

use super::model::ParsedCard;
use super::util::*;
use tarot_core::RawCardData;

pub fn parse(raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new("八达通 Octopus");
    card.currency = "HK$".to_string();

    if let Some(idm) = raw.get("felica_idm") {
        card.number = Some(idm.to_string());
    }

    if let Some(reader) = raw.get("felica_unsupported_reader") {
        card.notes.push(format!("不支持的读卡器：{reader}"));
        return card;
    }

    if let Some(block) = raw.get("felica_balance_block") {
        let bytes = hex_to_bytes(block);
        if let Some(bal) = extract_balance(&bytes) {
            card.balance = Some(bal);
        } else {
            card.notes.push("八达通余额块解析失败".into());
        }
    } else {
        card.notes.push("未取到八达通余额块".into());
    }

    card
}

/// 从 ACR FeliCa 响应中提取余额（港币元）。
///
/// Read Without Encryption 响应帧结构（对照 nfsee felica.js）：
/// `LEN 07 IDm(8) SF1 SF2 blockCount blockData(16)`，末尾可能带 PC/SC 的 90 00。
/// 定位响应码 0x07 后跳过 IDm(8)+状态标志(2)+块数(1)=12 字节即为块数据。
/// 块数据前 4 字节大端为余额原始值（read.js: `parseInt(slice(0,8),16)`），
/// `(值 - 500) * 10` 得单位 $0.01，故除 100 得港币元。
fn extract_balance(bytes: &[u8]) -> Option<f64> {
    // 去掉尾部 2 字节 SW（若存在且为 9000）。
    let body: &[u8] = if bytes.len() >= 2 && bytes[bytes.len() - 2] == 0x90 {
        &bytes[..bytes.len() - 2]
    } else {
        bytes
    };
    // 找 FeliCa 响应码 0x07。
    let pos = body.iter().position(|&b| b == 0x07)?;
    // 07 后：IDm(8) StatusFlag1 StatusFlag2 blockCount，随后为块数据。
    let data_start = pos + 12;
    if data_start + 4 > body.len() {
        return None;
    }
    // 大端前 4 字节。
    let raw_val = be_uint(body, data_start, data_start + 4);
    Some((raw_val as f64 - 500.0) * 10.0 / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_reader_is_reported_as_note() {
        let mut raw = RawCardData {
            card_type: "Octopus".into(),
            ..Default::default()
        };
        raw.put("felica_idm", "0102030405060708");
        raw.put("felica_unsupported_reader", "HID OMNIKEY 5022");

        let card = parse(&raw);

        assert_eq!(card.number.as_deref(), Some("0102030405060708"));
        assert!(card.fields.is_empty());
        assert_eq!(
            card.notes,
            vec!["不支持的读卡器：HID OMNIKEY 5022".to_string()]
        );
    }
}

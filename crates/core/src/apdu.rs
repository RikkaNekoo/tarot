//! APDU 命令与响应的通用类型。
//!
//! 后端使用这些类型构造命令、透传给读卡器并封装原始响应。
//! 解析交由前端完成，这里只关心字节与状态码。

use crate::error::{Error, Result};

/// 一条 APDU 命令（Command APDU）。
///
/// 结构遵循 ISO 7816-4：`CLA INS P1 P2 [Lc Data] [Le]`。
/// 对于 Mifare/FeliCa 的裸命令，直接放进 `raw` 字段整包发送。
#[derive(Debug, Clone)]
pub struct Apdu {
    bytes: Vec<u8>,
}

impl Apdu {
    /// 从原始字节构造（用于裸命令，如 Mifare `30xx`、FeliCa 帧）。
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self { bytes: bytes.into() }
    }

    /// 从十六进制字符串构造，如 `"00A4040000"`。
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s).map_err(|e| Error::Other(format!("bad hex apdu: {e}")))?;
        Ok(Self { bytes })
    }

    /// case 2：有 Le 无数据（`CLA INS P1 P2 Le`）。
    pub fn case2(cla: u8, ins: u8, p1: u8, p2: u8, le: u8) -> Self {
        Self { bytes: vec![cla, ins, p1, p2, le] }
    }

    /// case 3：有数据无 Le（`CLA INS P1 P2 Lc Data`）。
    pub fn case3(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Self {
        let mut b = vec![cla, ins, p1, p2, data.len() as u8];
        b.extend_from_slice(data);
        Self { bytes: b }
    }

    /// case 4：有数据有 Le（`CLA INS P1 P2 Lc Data Le`）。
    pub fn case4(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8], le: u8) -> Self {
        let mut b = vec![cla, ins, p1, p2, data.len() as u8];
        b.extend_from_slice(data);
        b.push(le);
        Self { bytes: b }
    }

    /// 底层字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 十六进制字符串，用于日志/APDU 追踪展示。
    pub fn to_hex(&self) -> String {
        hex::encode_upper(&self.bytes)
    }
}

/// 一条 APDU 响应（Response APDU）：数据体 + SW1SW2。
#[derive(Debug, Clone)]
pub struct ApduResponse {
    /// 去除 SW1SW2 后的数据体。
    pub data: Vec<u8>,
    pub sw1: u8,
    pub sw2: u8,
}

impl ApduResponse {
    /// 从读卡器返回的完整字节切分出数据与状态码。
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() < 2 {
            return Err(Error::ShortResponse(raw.len()));
        }
        let split = raw.len() - 2;
        Ok(Self {
            data: raw[..split].to_vec(),
            sw1: raw[split],
            sw2: raw[split + 1],
        })
    }

    /// 状态码合成为 u16，便于比较（如 `0x9000`）。
    pub fn sw(&self) -> u16 {
        ((self.sw1 as u16) << 8) | (self.sw2 as u16)
    }

    /// 是否为成功状态 `9000`。
    pub fn is_ok(&self) -> bool {
        self.sw() == 0x9000
    }

    /// 完整原始字节（数据 + SW1SW2），用于 APDU 追踪展示。
    pub fn to_hex(&self) -> String {
        let mut full = self.data.clone();
        full.push(self.sw1);
        full.push(self.sw2);
        hex::encode_upper(full)
    }

    /// 数据体的十六进制。
    pub fn data_hex(&self) -> String {
        hex::encode_upper(&self.data)
    }

    /// 若非成功状态则转为错误，便于 `?` 传播。
    pub fn ok_or_status(self) -> Result<Self> {
        if self.is_ok() {
            Ok(self)
        } else {
            Err(Error::ApduStatus { sw1: self.sw1, sw2: self.sw2 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_case2() {
        let a = Apdu::case2(0x80, 0x5C, 0x00, 0x02, 0x04);
        assert_eq!(a.to_hex(), "805C000204");
    }

    #[test]
    fn build_case3() {
        let a = Apdu::case3(0x00, 0xA4, 0x04, 0x00, &[0x11, 0x22]);
        assert_eq!(a.to_hex(), "00A404000211 22".replace(' ', ""));
    }

    #[test]
    fn build_case4() {
        let a = Apdu::case4(0x00, 0xA4, 0x04, 0x00, &[0xAA], 0x00);
        assert_eq!(a.to_hex(), "00A4040001AA00");
    }

    #[test]
    fn parse_response_ok() {
        let r = ApduResponse::parse(&[0x12, 0x34, 0x90, 0x00]).unwrap();
        assert_eq!(r.data, vec![0x12, 0x34]);
        assert_eq!(r.sw(), 0x9000);
        assert!(r.is_ok());
        assert_eq!(r.data_hex(), "1234");
        assert_eq!(r.to_hex(), "12349000");
    }

    #[test]
    fn parse_response_error_status() {
        let r = ApduResponse::parse(&[0x6A, 0x82]).unwrap();
        assert!(!r.is_ok());
        assert_eq!(r.sw(), 0x6A82);
        assert!(r.data.is_empty());
    }

    #[test]
    fn parse_too_short() {
        assert!(ApduResponse::parse(&[0x90]).is_err());
    }
}

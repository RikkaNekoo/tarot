//! BAC 后的安全消息通道（Secure Messaging，3DES 模式）。
//!
//! 包装底层 Transceiver：对每条命令 APDU 加密+MAC，对响应验签+解密，
//! 并维护随往返递增的 8 字节发送序列计数器（SSC）。
//! 规范：ICAO 9303 Part 11 §9.8。

use super::crypto;
use tarot_core::{Apdu, Error, Result, Transceiver};

/// BER-TLV 长度编码（短式/长式 0x81/0x82）。
pub fn encode_len(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else if len <= 0xFF {
        vec![0x81, len as u8]
    } else {
        vec![0x82, (len >> 8) as u8, (len & 0xFF) as u8]
    }
}

/// 解析 TLV 长度，返回 (长度, 消耗字节数)。从 data[pos] 起。
pub fn parse_len(data: &[u8], pos: usize) -> Result<(usize, usize)> {
    let first = *data.get(pos).ok_or_else(|| Error::Passport("TLV 长度越界".into()))?;
    if first < 0x80 {
        Ok((first as usize, 1))
    } else {
        let n = (first & 0x7F) as usize;
        if n == 0 || n > 2 || pos + 1 + n > data.len() {
            return Err(Error::Passport("不支持的 TLV 长度形式".into()));
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | data[pos + 1 + i] as usize;
        }
        Ok((len, 1 + n))
    }
}

/// 安全消息会话。
pub struct SecureSession {
    ks_enc: Vec<u8>,
    ks_mac: Vec<u8>,
    ssc: [u8; 8],
}

impl SecureSession {
    pub fn new(ks_enc: Vec<u8>, ks_mac: Vec<u8>, ssc: [u8; 8]) -> Self {
        Self { ks_enc, ks_mac, ssc }
    }

    /// SSC 视为大端整数 +1。
    fn increment_ssc(&mut self) {
        for i in (0..8).rev() {
            let (v, carry) = self.ssc[i].overflowing_add(1);
            self.ssc[i] = v;
            if !carry {
                break;
            }
        }
    }

    /// 经安全通道发送一条命令，返回解密后的明文数据与状态字。
    ///
    /// `data` 为命令数据体（可空），`le` 为期望响应长度（None 表示不带 Le）。
    /// 返回 (明文数据, sw1, sw2)。
    pub fn send<T: Transceiver>(
        &mut self,
        tx: &mut T,
        cla: u8,
        ins: u8,
        p1: u8,
        p2: u8,
        data: &[u8],
        le: Option<u8>,
    ) -> Result<(Vec<u8>, u8, u8)> {
        self.increment_ssc();
        let masked_cla = cla | 0x0C;
        let cmd_header = crypto::pad(&[masked_cla, ins, p1, p2]);

        // DO'87：加密数据（若有）
        let mut do87 = Vec::new();
        if !data.is_empty() {
            let enc = crypto::tdes_cbc_encrypt(&self.ks_enc, &crypto::pad(data));
            let mut val = vec![0x01u8];
            val.extend_from_slice(&enc);
            do87.push(0x87);
            do87.extend_from_slice(&encode_len(val.len()));
            do87.extend_from_slice(&val);
        }

        // DO'97：期望响应长度（若有 Le）
        let mut do97 = Vec::new();
        if let Some(le) = le {
            do97.extend_from_slice(&[0x97, 0x01, le]);
        }

        // MAC 输入 = pad(SSC || cmdHeader || DO87 || DO97)
        let mut mac_input = self.ssc.to_vec();
        mac_input.extend_from_slice(&cmd_header);
        mac_input.extend_from_slice(&do87);
        mac_input.extend_from_slice(&do97);
        let cc = crypto::retail_mac(&self.ks_mac, &crypto::pad(&mac_input));

        // DO'8E
        let mut do8e = vec![0x8E, 0x08];
        do8e.extend_from_slice(&cc);

        // 组装保护后数据体
        let mut body = Vec::new();
        body.extend_from_slice(&do87);
        body.extend_from_slice(&do97);
        body.extend_from_slice(&do8e);

        // 发送：maskedCLA INS P1 P2 Lc body 00
        let apdu = Apdu::case4(masked_cla, ins, p1, p2, &body, 0x00);
        let resp = tx.transceive(&apdu)?;

        self.increment_ssc();
        self.unprotect(&resp.data, resp.sw1, resp.sw2)
    }

    /// 验签并解密响应。
    fn unprotect(&self, data: &[u8], _sw1: u8, _sw2: u8) -> Result<(Vec<u8>, u8, u8)> {
        let mut do87 = Vec::new();
        let mut do99 = Vec::new();
        let mut do8e = Vec::new();
        let mut off = 0;
        while off < data.len() {
            let tag = data[off];
            off += 1;
            let (len, consumed) = parse_len(data, off)?;
            off += consumed;
            if off + len > data.len() {
                break;
            }
            let val = data[off..off + len].to_vec();
            off += len;
            match tag {
                0x87 => do87 = val,
                0x99 => do99 = val,
                0x8E => do8e = val,
                _ => {}
            }
        }

        if do99.len() != 2 {
            return Err(Error::Passport("响应缺少 DO'99 状态字".into()));
        }
        let (rsw1, rsw2) = (do99[0], do99[1]);

        // 验签
        if !do8e.is_empty() {
            let mut mac_input = self.ssc.to_vec();
            if !do87.is_empty() {
                mac_input.push(0x87);
                mac_input.extend_from_slice(&encode_len(do87.len()));
                mac_input.extend_from_slice(&do87);
            }
            mac_input.push(0x99);
            mac_input.push(0x02);
            mac_input.extend_from_slice(&do99);
            let cc = crypto::retail_mac(&self.ks_mac, &crypto::pad(&mac_input));
            if cc != do8e {
                return Err(Error::Passport("响应 MAC 校验失败".into()));
            }
        }

        // 解密 DO'87
        let mut plain = Vec::new();
        if !do87.is_empty() {
            if do87[0] != 0x01 {
                return Err(Error::Passport("DO'87 缺少填充指示字节".into()));
            }
            let dec = crypto::tdes_cbc_decrypt(&self.ks_enc, &do87[1..]);
            plain = crypto::unpad(&dec);
        }

        Ok((plain, rsw1, rsw2))
    }
}

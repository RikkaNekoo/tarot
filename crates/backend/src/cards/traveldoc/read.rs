//! 旅行证件（eMRTD）读取编排：SELECT 应用 → BAC 认证 → 安全通道读各 DG。
//!
//! 电子护照与电子往来港澳通行证共用同一套读取流程，后端不区分二者，
//! 统一以 `card_type = "TravelDoc"` 输出；具体是哪种证件由前端解析
//! DG1 的 MRZ 文档代码判定（护照 `P`、通行证 `C`）。
//!
//! 后端负责完成 BAC 与安全消息通道（有状态，需即时加解密），
//! 把解密后的各数据组明文以 `passport_*` 键存入 `RawCardData.raw_fields`，
//! 语义解析（MRZ 字段、人脸图像）交前端完成。
//!
//! 规范：ICAO Doc 9303 Part 10/11。参考实现：`cenfc/`。

use super::crypto;
use super::sm::{self, SecureSession};
use rand::RngCore;
use tarot_core::{Apdu, Error, PassportKey, RawCardData, Result, Transceiver};

/// eMRTD 应用 AID：A0 00 00 02 47 10 01
const EMRTD_AID: [u8; 7] = [0xA0, 0x00, 0x00, 0x02, 0x47, 0x10, 0x01];

/// 每次 READ BINARY 的最大块长（保守值，兼容性好）。
const MAX_READ: usize = 0xA0;

/// 要读取的数据组：(键名, 文件 ID 高字节, 低字节)。
const DATA_GROUPS: &[(&str, u8, u8)] = &[
    ("passport_com", 0x01, 0x1E),
    ("passport_dg1", 0x01, 0x01),
    ("passport_dg2", 0x01, 0x02),
    ("passport_dg11", 0x01, 0x0B),
    ("passport_dg12", 0x01, 0x0C),
    ("passport_sod", 0x01, 0x1D),
];

/// 读取旅行证件：完成 BAC，通过安全通道读各 DG，明文写入 `data`。
///
/// 护照与港澳通行证流程一致，此处不做区分，统一标记为 `TravelDoc`。
pub fn read_traveldoc<T: Transceiver>(
    tx: &mut T,
    key: &PassportKey,
    data: &mut RawCardData,
) -> Result<()> {
    key.validate().map_err(Error::Passport)?;
    data.card_type = "TravelDoc".into();

    // 1. SELECT eMRTD 应用
    let sel = Apdu::case4(0x00, 0xA4, 0x04, 0x0C, &EMRTD_AID, 0x00);
    let r = tx.transceive(&sel)?;
    if !r.is_ok() {
        return Err(Error::Passport(format!(
            "SELECT eMRTD 失败: SW={:02X}{:02X}",
            r.sw1, r.sw2
        )));
    }

    // 2. BAC 认证，建立安全通道
    let mut session = perform_bac(tx, key)?;
    data.sub_cards.push("TravelDoc".into());

    // 3. 通过安全通道读各 DG
    for (name, hi, lo) in DATA_GROUPS {
        match read_data_group(tx, &mut session, *hi, *lo) {
            Ok(bytes) if !bytes.is_empty() => {
                data.put(*name, hex::encode_upper(&bytes));
            }
            _ => {
                // DG 不存在或读取失败：跳过，继续读其余 DG。
            }
        }
    }

    Ok(())
}

/// 生成 n 字节随机数。
fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut v);
    v
}

/// 执行 BAC 认证，成功后返回安全消息会话。
fn perform_bac<T: Transceiver>(tx: &mut T, key: &PassportKey) -> Result<SecureSession> {
    let mrz_key = key.mrz_key();
    let kseed = crypto::generate_kseed(&mrz_key);
    let kenc = crypto::derive_key(&kseed, 1);
    let kmac = crypto::derive_key(&kseed, 2);

    // GET CHALLENGE：取 8 字节 rnd.icc
    let gc = Apdu::case2(0x00, 0x84, 0x00, 0x00, 0x08);
    let r = tx.transceive(&gc)?;
    if !r.is_ok() || r.data.len() != 8 {
        return Err(Error::Passport(format!(
            "GET CHALLENGE 失败: SW={:02X}{:02X} 长度={}",
            r.sw1,
            r.sw2,
            r.data.len()
        )));
    }
    let rnd_icc = r.data.clone();

    // 终端随机
    let rnd_ifd = random_bytes(8);
    let k_ifd = random_bytes(16);

    // S = rnd.ifd || rnd.icc || k.ifd
    let mut s = Vec::with_capacity(32);
    s.extend_from_slice(&rnd_ifd);
    s.extend_from_slice(&rnd_icc);
    s.extend_from_slice(&k_ifd);

    // Eifd = 3DES-CBC(Kenc, S)；Mifd = RetailMAC(Kmac, pad(Eifd))
    let eifd = crypto::tdes_cbc_encrypt(&kenc, &s);
    let mifd = crypto::retail_mac(&kmac, &crypto::pad(&eifd));

    // MUTUAL AUTHENTICATE(Eifd || Mifd)
    let mut auth_data = eifd.clone();
    auth_data.extend_from_slice(&mifd);
    let ma = Apdu::case4(0x00, 0x82, 0x00, 0x00, &auth_data, 0x28);
    let r = tx.transceive(&ma)?;
    if !r.is_ok() || r.data.len() != 40 {
        return Err(Error::Passport(format!(
            "MUTUAL AUTHENTICATE 失败: SW={:02X}{:02X} 长度={}",
            r.sw1,
            r.sw2,
            r.data.len()
        )));
    }

    // 响应 = 加密(32) || MAC(8)
    let resp_enc = &r.data[..32];
    let resp_mac = &r.data[32..40];
    let computed = crypto::retail_mac(&kmac, &crypto::pad(resp_enc));
    if computed != resp_mac {
        return Err(Error::Passport("响应 MAC 校验失败".into()));
    }

    // 解密 → rnd.icc' || rnd.ifd' || k.icc
    let dec = crypto::tdes_cbc_decrypt(&kenc, resp_enc);
    if dec.len() != 32 {
        return Err(Error::Passport("BAC 响应解密长度异常".into()));
    }
    let rnd_icc2 = &dec[0..8];
    let rnd_ifd2 = &dec[8..16];
    let k_icc = &dec[16..32];
    if rnd_icc2 != rnd_icc.as_slice() {
        return Err(Error::Passport("rnd.icc 不匹配".into()));
    }
    if rnd_ifd2 != rnd_ifd.as_slice() {
        return Err(Error::Passport("rnd.ifd 不匹配（MRZ 密钥可能错误）".into()));
    }

    // 会话密钥：Kseed_new = k.ifd XOR k.icc
    let mut kseed_new = [0u8; 16];
    for i in 0..16 {
        kseed_new[i] = k_ifd[i] ^ k_icc[i];
    }
    let ks_enc = crypto::derive_key(&kseed_new, 1);
    let ks_mac = crypto::derive_key(&kseed_new, 2);

    // SSC = rnd.icc[4..8] || rnd.ifd[4..8]
    let mut ssc = [0u8; 8];
    ssc[..4].copy_from_slice(&rnd_icc[4..8]);
    ssc[4..].copy_from_slice(&rnd_ifd[4..8]);

    Ok(SecureSession::new(ks_enc, ks_mac, ssc))
}

/// 经安全通道 SELECT EF 并读取整个文件内容（明文）。
fn read_data_group<T: Transceiver>(
    tx: &mut T,
    session: &mut SecureSession,
    hi: u8,
    lo: u8,
) -> Result<Vec<u8>> {
    // SELECT EF by file ID：00 A4 02 0C 02 <hi lo>
    let (_d, sw1, sw2) = session.send(tx, 0x00, 0xA4, 0x02, 0x0C, &[hi, lo], None)?;
    if (sw1, sw2) != (0x90, 0x00) {
        return Err(Error::Passport(format!(
            "SELECT EF {hi:02X}{lo:02X} 失败: SW={sw1:02X}{sw2:02X}"
        )));
    }
    read_binary_all(tx, session)
}

/// READ BINARY 读取当前选中 EF 的全部内容。
/// 先读前 4 字节确定 TLV 总长，再分块读。
fn read_binary_all<T: Transceiver>(tx: &mut T, session: &mut SecureSession) -> Result<Vec<u8>> {
    // 读前 4 字节 header
    let (header, sw1, sw2) = session.send(tx, 0x00, 0xB0, 0x00, 0x00, &[], Some(0x04))?;
    if (sw1, sw2) != (0x90, 0x00) || header.is_empty() {
        return Err(Error::Passport(format!(
            "READ BINARY header 失败: SW={sw1:02X}{sw2:02X}"
        )));
    }

    let total = tlv_total_len(&header)?;
    let mut file = header.clone();

    while file.len() < total {
        let offset = file.len();
        let remaining = total - offset;
        let le = remaining.min(MAX_READ) as u8;
        let p1 = (offset >> 8) as u8;
        let p2 = (offset & 0xFF) as u8;
        let (chunk, sw1, sw2) = session.send(tx, 0x00, 0xB0, p1, p2, &[], Some(le))?;
        if sw1 == 0x6B && sw2 == 0x00 {
            break; // 越界
        }
        if (sw1, sw2) != (0x90, 0x00) || chunk.is_empty() {
            break;
        }
        let got = chunk.len();
        file.extend_from_slice(&chunk);
        if got < le as usize {
            break; // 短读，到末尾
        }
    }

    Ok(file)
}

/// 从 TLV 头部（tag + length）算出整个对象的总字节数。
fn tlv_total_len(header: &[u8]) -> Result<usize> {
    // tag：1 或 2 字节（首字节低 5 位全 1 表示多字节 tag）
    let mut pos = 1;
    if header[0] & 0x1F == 0x1F {
        // 多字节 tag，跳过后续 tag 字节
        while pos < header.len() && header[pos] & 0x80 != 0 {
            pos += 1;
        }
        pos += 1;
    }
    let (content_len, len_bytes) = sm::parse_len(header, pos)?;
    Ok(pos + len_bytes + content_len)
}

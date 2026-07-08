//! 护照 BAC 所需的密码学原语（ICAO 9303 Part 11）。
//!
//! 用 RustCrypto 的 `des` 实现 DES/3DES-CBC/ECB、Retail-MAC、
//! ICAO 密钥派生与 ISO 9797-1 Method 2 填充。

use cbc::cipher::block_padding::NoPadding as NoPad;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};
use des::{Des, TdesEde2};
use sha1::{Digest, Sha1};

type DesCbcEnc = cbc::Encryptor<Des>;
type DesEcbEnc = ecb::Encryptor<Des>;
type DesEcbDec = ecb::Decryptor<Des>;
type TdesCbcEnc = cbc::Encryptor<TdesEde2>;
type TdesCbcDec = cbc::Decryptor<TdesEde2>;

/// ISO 9797-1 Method 2 填充：追加 0x80 后补 0x00 到 8 字节倍数。
pub fn pad(data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    v.push(0x80);
    while v.len() % 8 != 0 {
        v.push(0x00);
    }
    v
}

/// 去除 Method 2 填充（找最后一个 0x80，其后须全为 0x00）。
pub fn unpad(data: &[u8]) -> Vec<u8> {
    let mut i = data.len();
    while i > 0 {
        i -= 1;
        match data[i] {
            0x00 => continue,
            0x80 => return data[..i].to_vec(),
            _ => break,
        }
    }
    data.to_vec()
}

/// 3DES-CBC 加密（2-key，IV=0，无填充，输入须 8 字节倍数）。
pub fn tdes_cbc_encrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    let iv = [0u8; 8];
    let mut buf = data.to_vec();
    let enc = TdesCbcEnc::new_from_slices(key, &iv).expect("3des key/iv");
    enc.encrypt_padded_mut::<NoPad>(&mut buf, data.len())
        .expect("cbc enc")
        .to_vec()
}

/// 3DES-CBC 解密。
pub fn tdes_cbc_decrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    let iv = [0u8; 8];
    let mut buf = data.to_vec();
    let dec = TdesCbcDec::new_from_slices(key, &iv).expect("3des key/iv");
    dec.decrypt_padded_mut::<NoPad>(&mut buf)
        .expect("cbc dec")
        .to_vec()
}

/// 单 DES-CBC 加密（8 字节 key，IV=0）。
fn des_cbc_encrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    let iv = [0u8; 8];
    let mut buf = data.to_vec();
    let enc = DesCbcEnc::new_from_slices(key, &iv).expect("des key/iv");
    enc.encrypt_padded_mut::<NoPad>(&mut buf, data.len())
        .expect("des cbc enc")
        .to_vec()
}

/// 单 DES-ECB 加密单块。
fn des_ecb_encrypt(key: &[u8], block: &[u8]) -> Vec<u8> {
    let mut buf = block.to_vec();
    let enc = DesEcbEnc::new_from_slice(key).expect("des key");
    enc.encrypt_padded_mut::<NoPad>(&mut buf, block.len())
        .expect("des ecb enc")
        .to_vec()
}

/// 单 DES-ECB 解密单块。
fn des_ecb_decrypt(key: &[u8], block: &[u8]) -> Vec<u8> {
    let mut buf = block.to_vec();
    let dec = DesEcbDec::new_from_slice(key).expect("des key");
    dec.decrypt_padded_mut::<NoPad>(&mut buf)
        .expect("des ecb dec")
        .to_vec()
}

/// ISO 9797-1 MAC Algorithm 3（Retail MAC）。
/// key 16 字节拆 Ka/Kb；message 须已 Method 2 填充到 8 字节倍数，返回 8 字节。
pub fn retail_mac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let ka = &key[..8];
    let kb = &key[8..16];
    let cbc = des_cbc_encrypt(ka, message);
    let last = &cbc[cbc.len() - 8..];
    let y = des_ecb_decrypt(kb, last);
    des_ecb_encrypt(ka, &y)
}

/// Kseed = SHA1(mrz_key)[0..16]。
pub fn generate_kseed(mrz_key: &str) -> Vec<u8> {
    let mut h = Sha1::new();
    h.update(mrz_key.as_bytes());
    h.finalize()[..16].to_vec()
}

/// ICAO 密钥派生：mode=1 加密(Kenc/KSenc)，mode=2 MAC(Kmac/KSmac)。
/// 返回 16 字节 2-key 3DES 密钥（已奇校验）。
pub fn derive_key(kseed: &[u8], mode: u8) -> Vec<u8> {
    let mut input = kseed.to_vec();
    input.extend_from_slice(&[0x00, 0x00, 0x00, mode]);
    let mut h = Sha1::new();
    h.update(&input);
    let hash = h.finalize();
    let mut key = adjust_parity(&hash[0..8]);
    key.extend_from_slice(&adjust_parity(&hash[8..16]));
    key
}

/// DES 奇校验：调整每字节最低位使其 1 的个数为奇。
fn adjust_parity(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|&b| {
            if (b.count_ones() % 2) == 0 {
                b ^ 0x01
            } else {
                b
            }
        })
        .collect()
}

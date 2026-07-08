//! 最小 BER-TLV 解析器，供 EMV FCI / 模板解析使用。
//! 只做定位与提取，不校验完整语义。

/// 一个 TLV 节点。
#[derive(Debug, Clone)]
pub struct Tlv {
    /// tag 的十六进制（大写），如 "57"、"9F38"、"BF0C"。
    pub tag: String,
    /// 值字节。
    pub value: Vec<u8>,
    /// 若为构造类型（tag 第 6 位为 1），其子节点。
    pub children: Vec<Tlv>,
}

/// 解析一段字节为 TLV 列表（同一层多个节点）。
pub fn parse(bytes: &[u8]) -> Vec<Tlv> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // 跳过填充字节 00 / FF。
        if bytes[i] == 0x00 || bytes[i] == 0xFF {
            i += 1;
            continue;
        }
        // --- 解析 tag ---
        let first = bytes[i];
        let constructed = first & 0x20 != 0;
        let mut tag_bytes = vec![first];
        i += 1;
        // 多字节 tag：首字节低 5 位全 1。
        if first & 0x1F == 0x1F {
            while i < bytes.len() {
                let b = bytes[i];
                tag_bytes.push(b);
                i += 1;
                if b & 0x80 == 0 {
                    break;
                }
            }
        }
        // --- 解析 length ---
        if i >= bytes.len() {
            break;
        }
        let mut len = bytes[i] as usize;
        i += 1;
        if len & 0x80 != 0 {
            let num = len & 0x7F;
            len = 0;
            for _ in 0..num {
                if i >= bytes.len() {
                    break;
                }
                len = (len << 8) | bytes[i] as usize;
                i += 1;
            }
        }
        // --- 取值 ---
        let end = (i + len).min(bytes.len());
        let value = bytes[i..end].to_vec();
        i = end;

        let children = if constructed { parse(&value) } else { Vec::new() };
        out.push(Tlv {
            tag: hex::encode_upper(&tag_bytes),
            value,
            children,
        });
    }
    out
}

/// 在 TLV 树中递归查找首个匹配 tag 的节点值。
pub fn find<'a>(nodes: &'a [Tlv], tag: &str) -> Option<&'a Tlv> {
    for n in nodes {
        if n.tag == tag {
            return Some(n);
        }
        if let Some(found) = find(&n.children, tag) {
            return Some(found);
        }
    }
    None
}
//! 各卡种识别用的 AID / 命令常量，源自 nfsee `read.js` 的探测链。

/// 交通/支付卡的 SELECT AID 探测表（Type A 探测链）。
/// 元素：(卡类型名, AID 字节)。
pub struct AidEntry {
    pub name: &'static str,
    pub aid: &'static [u8],
    /// 抓取 FCI 时使用的字段键。
    pub key: &'static str,
}

/// PPSE 目录名 `2PAY.SYS.DDF01`（EMV 非接目录）。
pub const PPSE: &[u8] = b"2PAY.SYS.DDF01";

/// Type A 探测链（顺序与 nfsee `ReadAnyCard` 一致）。
pub const TYPE_A_CHAIN: &[AidEntry] = &[
    AidEntry {
        name: "ShenzhenTong",
        aid: b"PAY.SZT",
        key: "szt_fci",
    },
    AidEntry {
        name: "WuhanTong",
        aid: &[0x41, 0x50, 0x31, 0x2e, 0x57, 0x48, 0x43, 0x54, 0x43],
        key: "whctc_fci",
    },
    AidEntry {
        name: "LingnanPass",
        aid: b"PAY.APPY",
        key: "lnt_fci",
    },
    AidEntry {
        name: "TMoney",
        aid: &[0xD4, 0x10, 0x00, 0x00, 0x03, 0x00, 0x01],
        key: "tmoney_fci",
    },
    AidEntry {
        name: "MacauPass",
        aid: &[0xB0, 0xC4, 0xC3, 0xC5, 0xCD, 0xA8, 0xC7, 0xAE, 0xB0, 0xFC],
        key: "macau_fci",
    },
    AidEntry {
        name: "MotBmac",
        aid: &[
            0x91, 0x56, 0x00, 0x00, 0x14, 0x4D, 0x4F, 0x54, 0x2E, 0x42, 0x4D, 0x41, 0x43, 0x30,
            0x30, 0x31,
        ],
        key: "mot_bmac_fci",
    },
    AidEntry {
        name: "ChinaTransit",
        aid: &[
            0xD1, 0x56, 0x00, 0x00, 0x15, 0xB9, 0xAB, 0xB9, 0xB2, 0xD3, 0xA6, 0xD3, 0xC3,
        ],
        key: "china_transit_fci",
    },
    AidEntry {
        name: "SuXin",
        aid: b"SUXIN.DDF01",
        key: "suxin_fci",
    },
    AidEntry {
        name: "SzpkZyy",
        aid: b"SZPK_ZYY",
        key: "szpk_zyy_fci",
    },
    AidEntry {
        name: "CityUnion",
        aid: &[0xA0, 0x00, 0x00, 0x00, 0x03, 0x86, 0x98, 0x07, 0x01],
        key: "cityunion_fci",
    },
    AidEntry {
        name: "TUnion",
        aid: &[0xA0, 0x00, 0x00, 0x06, 0x32, 0x01, 0x01, 0x05],
        key: "tunion_fci",
    },
];

/// Macau Pass 的 AMTJAVACARD 容器 AID（备用路径）。
pub const AMTJAVACARD: &[u8] = &[
    0x41, 0x4D, 0x54, 0x4A, 0x41, 0x56, 0x41, 0x43, 0x41, 0x52, 0x44,
];

/// 岭南通 PAY.TICL 二次 SELECT。
pub const LINGNAN_TICL: &[u8] = b"PAY.TICL";

/// EMV AID 前缀到品牌名映射（用于识别 PPSE 选出的应用）。
pub const EMV_AID_NAMES: &[(&str, &str)] = &[
    ("A000000333010101", "UnionPay-Debit"),
    ("A000000333010102", "UnionPay-Credit"),
    ("A000000333010103", "UnionPay-SecuredCredit"),
    ("A000000003", "Visa"),
    ("A000000004", "MasterCard"),
    ("A000000010", "MasterCard-China"),
    ("A000000025", "AMEX"),
    ("A000000065", "JCB"),
    ("A000000324", "Discover"),
    ("A000000790", "AMEX-China"),
];

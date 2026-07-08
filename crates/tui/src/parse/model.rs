//! 前端解析后的人类可读数据模型。UI 层只消费这些结构，不碰原始字节。

/// 一条交易记录（PBOC / T-Money 等）。
///
/// 对于交通联合，会把 SFI 0x1E 行程记录里对应序号的信息（进出站、辅助类型、
/// 交易后余额、时间戳、线路站点）合并进来，一条记录即一次完整交易。
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    /// 来源协议（多协议交通卡合并显示时使用）。
    pub source: String,
    /// 序号（若有）。
    pub seq: Option<u64>,
    /// 交易类型可读名（消费/圈存/充值等）。
    pub kind: String,
    /// 金额（元），带符号。
    pub amount: f64,
    /// 日期字符串 YYYY-MM-DD（若有）。
    pub date: String,
    /// 时间字符串 HH:MM:SS（若有）。
    pub time: String,
    /// 终端号 hex（若有）。
    pub terminal: String,
    /// 行程辅助类型（地铁/公交，仅交通联合行程记录有）。
    pub aux: String,
    /// 进出站类型（进站/出站/单次，仅交通联合行程记录有）。
    pub trip_kind: String,
    /// 交易后余额（元，仅行程记录有）。
    pub balance_after: Option<f64>,
    /// 线路和站点 hex（仅行程记录有）。
    pub line_station: String,
}

/// 一条行程记录（交通联合 SFI 0x1E）。
#[derive(Debug, Clone, Default)]
pub struct Trip {
    /// 交易类型可读名（进站/出站/单次等）。
    pub kind: String,
    /// 辅助类型（地铁/公交/其他）。
    pub aux: String,
    /// 交易金额（元）。
    pub amount: f64,
    /// 交易后余额（元）。
    pub balance: f64,
    /// 时间戳 `YYYY-MM-DD HH:MM:SS`。
    pub timestamp: String,
    /// 线路和站点 hex。
    pub line_station: String,
    /// 城市码 hex。
    pub city: String,
}

/// 键值对形式的附加信息（有效期、发行日期、城市等）。
pub type Field = (String, String);

/// 多协议交通卡里单个协议的摘要信息。
#[derive(Debug, Clone, Default)]
pub struct ProtocolSection {
    /// 协议可读名。
    pub name: String,
    /// 卡号 / PAN（若有）。
    pub number: Option<String>,
    /// 余额（元），若适用。
    pub balance: Option<f64>,
    /// 货币符号。
    pub currency: String,
    /// 附加字段。
    pub fields: Vec<Field>,
    /// 解析告警。
    pub notes: Vec<String>,
}

/// 单张（子）卡解析结果。
#[derive(Debug, Clone, Default)]
pub struct ParsedCard {
    /// 卡种可读名（如 "深圳通"、"Visa"）。
    pub name: String,
    /// 卡号 / PAN（若有）。
    pub number: Option<String>,
    /// 余额（元），若适用。
    pub balance: Option<f64>,
    /// 货币符号（默认 ¥，八达通 HK$）。
    pub currency: String,
    /// 附加字段（有效期、品牌、城市、版本等）。
    pub fields: Vec<Field>,
    /// 交易记录列表（交通联合会把 SFI 0x1E 行程信息合并进对应记录）。
    pub transactions: Vec<Transaction>,
    /// 多协议交通卡的各协议信息，按读取顺序排列。
    pub protocols: Vec<ProtocolSection>,
    /// 解析过程中的告警（字段缺失、长度异常等）。
    pub notes: Vec<String>,
}

impl ParsedCard {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            currency: "¥".to_string(),
            ..Default::default()
        }
    }

    pub fn add_field(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.fields.push((k.into(), v.into()));
    }
}

/// 整卡解析结果（可能含多张叠加子卡）。
#[derive(Debug, Clone, Default)]
pub struct ParsedResult {
    /// 顶层 card_type（如 "CombinedCard"）。
    pub card_type: String,
    /// ATR hex。
    pub atr: String,
    /// 各子卡解析结果。
    pub cards: Vec<ParsedCard>,
}

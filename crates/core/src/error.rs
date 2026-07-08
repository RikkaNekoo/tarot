//! 统一错误类型，覆盖读卡器、卡片与 APDU 层的各种失败场景。

use thiserror::Error;

/// 项目通用错误类型。
#[derive(Debug, Error)]
pub enum Error {
    /// 底层 PC/SC 栈返回的错误（读卡器驱动、连接等）。
    #[error("PC/SC error: {0}")]
    Pcsc(String),

    /// 未检测到任何读卡器。
    #[error("no reader available")]
    NoReader,

    /// 读卡器上当前没有卡片。
    #[error("no card present in reader")]
    NoCard,

    /// 卡片在操作过程中被移开或复位。
    #[error("card was removed or reset")]
    CardRemoved,

    /// 卡片返回了非 9000 的状态码（SW1SW2）。
    #[error("card returned status {sw1:02X}{sw2:02X}")]
    ApduStatus { sw1: u8, sw2: u8 },

    /// 收到的响应长度不足以解析。
    #[error("response too short: {0} bytes")]
    ShortResponse(usize),

    /// 卡片类型无法识别。
    #[error("unsupported or unknown card")]
    UnknownCard,

    /// 护照读取失败（BAC / 安全消息 / 数据组读取）。
    #[error("passport error: {0}")]
    Passport(String),

    /// 其他内部错误。
    #[error("{0}")]
    Other(String),
}

/// 便捷 Result 别名。
pub type Result<T> = std::result::Result<T, Error>;
//! `tarot-core`：前后端共享的基础类型。
//!
//! 包含 APDU 命令/响应、卡片原始数据模型、传输抽象与统一错误类型。
//! 该 crate 不依赖 PC/SC 或 TUI，可独立测试。

pub mod apdu;
pub mod error;
pub mod model;
pub mod passport;

pub use apdu::{Apdu, ApduResponse};
pub use error::{Error, Result};
pub use model::{ApduTrace, CardStandard, RawCardData, Transceiver};
pub use passport::PassportKey;

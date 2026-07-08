//! 旅行证件（eMRTD）读取。
//!
//! 电子护照与电子往来港澳通行证共享同一套 ICAO Doc 9303 机制：
//! 相同的 eMRTD 应用 AID、相同的 BAC 认证、相同的安全消息通道、
//! 相同的 MRZ 密钥合成与数据组（DG）文件布局。因此后端不做区分，
//! 统一读取并标记为 `TravelDoc`；究竟是护照还是通行证，由前端解析
//! DG1 内 MRZ 的文档代码判定。

pub mod crypto;
pub mod read;
pub mod sm;

pub use read::read_traveldoc;

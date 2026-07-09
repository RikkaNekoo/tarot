//! PC/SC 读卡器层：枚举读卡器、监控卡片状态、建立连接并透传 APDU。
//!
//! Mifare/FeliCa 的裸命令由上层 `cards` 模块封装为 ACR 伪 APDU 后再经此发送。

use pcsc::{Card, Context, Protocols, Scope, ShareMode, MAX_BUFFER_SIZE};
use std::ffi::CString;
use tarot_core::{Apdu, ApduResponse, ApduTrace, Error, Result, Transceiver};

/// 读卡器上的卡片状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardStatus {
    /// 读卡器存在但未插卡。
    Empty,
    /// 卡片已就位，可连接。
    Present,
}

/// PC/SC 管理器：持有 context，负责枚举与状态查询。
pub struct PcscManager {
    ctx: Context,
}

impl PcscManager {
    /// 建立与 PC/SC 守护进程（pcscd）的连接。
    pub fn new() -> Result<Self> {
        let ctx = Context::establish(Scope::User)
            .map_err(|e| Error::Pcsc(format!("establish context: {e}")))?;
        Ok(Self { ctx })
    }

    /// 列出当前所有读卡器名称。
    pub fn list_readers(&self) -> Result<Vec<String>> {
        let mut buf = vec![0u8; 2048];
        let readers = self
            .ctx
            .list_readers(&mut buf)
            .map_err(|e| Error::Pcsc(format!("list readers: {e}")))?;
        let names: Vec<String> = readers.map(|r| r.to_string_lossy().into_owned()).collect();
        Ok(names)
    }

    /// 返回第一个可用读卡器名称，没有则报 `NoReader`。
    pub fn first_reader(&self) -> Result<String> {
        self.list_readers()?
            .into_iter()
            .next()
            .ok_or(Error::NoReader)
    }

    /// 尝试连接指定读卡器上的卡片。若无卡返回 `NoCard`。
    pub fn connect(&self, reader: &str) -> Result<CardSession> {
        let c_reader =
            CString::new(reader).map_err(|e| Error::Other(format!("bad reader name: {e}")))?;
        match self
            .ctx
            .connect(&c_reader, ShareMode::Shared, Protocols::ANY)
        {
            Ok(card) => Ok(CardSession::new(card)),
            Err(pcsc::Error::NoSmartcard) | Err(pcsc::Error::RemovedCard) => Err(Error::NoCard),
            Err(e) => Err(Error::Pcsc(format!("connect: {e}"))),
        }
    }

    /// 查询读卡器当前卡片状态（不建立连接），用于 TUI 状态监控。
    pub fn status(&self, reader: &str) -> Result<CardStatus> {
        use pcsc::{ReaderState, State};
        let c_reader =
            CString::new(reader).map_err(|e| Error::Other(format!("bad reader name: {e}")))?;
        let mut states = [ReaderState::new(c_reader, State::UNAWARE)];
        self.ctx
            .get_status_change(std::time::Duration::from_millis(0), &mut states)
            .map_err(|e| Error::Pcsc(format!("status change: {e}")))?;
        let s = states[0].event_state();
        if s.contains(State::PRESENT) {
            Ok(CardStatus::Present)
        } else {
            Ok(CardStatus::Empty)
        }
    }
}

/// 一次已建立的卡片会话，实现 [`Transceiver`]，并累积 APDU 历史。
pub struct CardSession {
    card: Card,
    /// APDU 交互历史，供前端追踪展示。
    pub history: Vec<ApduTrace>,
}

impl CardSession {
    fn new(card: Card) -> Self {
        Self {
            card,
            history: Vec::new(),
        }
    }

    /// 读取卡片 ATR（Answer To Reset）原始字节。
    pub fn atr(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 64];
        let atr = self
            .card
            .get_attribute(pcsc::Attribute::AtrString, &mut buf)
            .map_err(|e| Error::Pcsc(format!("get atr: {e}")))?;
        Ok(atr.to_vec())
    }

    /// ATR 的十六进制表示。
    pub fn atr_hex(&self) -> Result<String> {
        Ok(hex::encode_upper(self.atr()?))
    }

    /// 消费会话，取出完整 APDU 历史。
    pub fn into_history(self) -> Vec<ApduTrace> {
        self.history
    }
}

impl CardSession {
    /// 向卡片发送一次原始 APDU，返回原始响应字节。
    ///
    /// 遇到卡片复位（ResetCard，常见于安全芯片收到非法命令后自复位）时，
    /// 自动 `reconnect` 重新上电并重试一次，避免整个读卡流程因一条命令中断。
    /// 真正被移除（RemovedCard）则返回 `CardRemoved`。
    fn raw_transmit(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
        let mut rx = vec![0u8; MAX_BUFFER_SIZE];
        match self.card.transmit(apdu, &mut rx) {
            Ok(r) => Ok(r.to_vec()),
            Err(pcsc::Error::ResetCard) => {
                // 卡片复位：重新上电后重试一次。
                self.card
                    .reconnect(
                        ShareMode::Shared,
                        Protocols::ANY,
                        pcsc::Disposition::ResetCard,
                    )
                    .map_err(|e| Error::Pcsc(format!("reconnect: {e}")))?;
                let mut rx2 = vec![0u8; MAX_BUFFER_SIZE];
                match self.card.transmit(apdu, &mut rx2) {
                    Ok(r) => Ok(r.to_vec()),
                    Err(pcsc::Error::RemovedCard) | Err(pcsc::Error::ResetCard) => {
                        Err(Error::CardRemoved)
                    }
                    Err(e) => Err(Error::Pcsc(format!("transmit after reconnect: {e}"))),
                }
            }
            Err(pcsc::Error::RemovedCard) => Err(Error::CardRemoved),
            Err(e) => Err(Error::Pcsc(format!("transmit: {e}"))),
        }
    }
}

impl Transceiver for CardSession {
    fn transceive(&mut self, apdu: &Apdu) -> Result<ApduResponse> {
        let raw = self.raw_transmit(apdu.as_bytes())?;

        // 空响应：某些卡对不支持的指令会返回 0 字节。
        // 转为哨兵状态 SW=0000（视为“失败但非致命”），让探测链继续而非中断。
        if raw.len() < 2 {
            let parsed = ApduResponse {
                data: vec![],
                sw1: 0x00,
                sw2: 0x00,
            };
            self.history.push(ApduTrace {
                tx: apdu.to_hex(),
                rx: hex::encode_upper(&raw),
                note: Some("empty/short response".into()),
            });
            return Ok(parsed);
        }

        let mut parsed = ApduResponse::parse(&raw)?;

        // 6CXX：Le 长度不对，卡片告知正确长度 XX，用它重发（case 2）。
        if parsed.sw1 == 0x6C && apdu.as_bytes().len() >= 4 {
            let cmd = apdu.as_bytes();
            let retry = [cmd[0], cmd[1], cmd[2], cmd[3], parsed.sw2];
            let raw2 = self.raw_transmit(&retry)?;
            if raw2.len() >= 2 {
                parsed = ApduResponse::parse(&raw2)?;
            }
        }

        // 61XX：还有 XX 字节待取，循环 GET RESPONSE (00 C0 00 00 XX) 拼接数据。
        while parsed.sw1 == 0x61 {
            let le = parsed.sw2;
            let get_resp = [0x00, 0xC0, 0x00, 0x00, le];
            let raw2 = self.raw_transmit(&get_resp)?;
            if raw2.len() < 2 {
                break;
            }
            let next = ApduResponse::parse(&raw2)?;
            // 把已有数据与新数据拼接，SW 用最新一段的。
            let mut merged = parsed.data.clone();
            merged.extend_from_slice(&next.data);
            parsed = ApduResponse {
                data: merged,
                sw1: next.sw1,
                sw2: next.sw2,
            };
        }

        // 记录到历史（成功与失败都记，便于调试）。
        self.history.push(ApduTrace {
            tx: apdu.to_hex(),
            rx: parsed.to_hex(),
            note: None,
        });
        Ok(parsed)
    }
}

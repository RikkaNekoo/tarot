//! TUI 应用状态：读卡器管理、卡片状态轮询、读卡结果与解析、滚动/焦点。

use crate::parse::{self, ParsedResult};
use tarot_backend::reader::{CardStatus, PcscManager};
use tarot_backend::{read_from_reader, read_traveldoc_from_reader};
use tarot_core::{ApduTrace, Error, PassportKey, RawCardData};
use std::time::{Duration, Instant};

/// 旅行证件 MRZ 输入的三个字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelDocField {
    DocNumber,
    Dob,
    Doe,
}

/// 旅行证件输入态：正在录入 MRZ 三要素。
#[derive(Debug, Clone, Default)]
pub struct TravelDocInput {
    pub doc_number: String,
    pub dob: String,
    pub doe: String,
}

impl TravelDocInput {
    fn field_mut(&mut self, f: TravelDocField) -> &mut String {
        match f {
            TravelDocField::DocNumber => &mut self.doc_number,
            TravelDocField::Dob => &mut self.dob,
            TravelDocField::Doe => &mut self.doe,
        }
    }
}

/// 当前焦点区域（决定方向键/滚动作用于谁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Readers,
    Parsed,
    Apdu,
}

/// 读卡状态机。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadState {
    /// 空闲，未放卡或已取卡。
    Idle,
    /// 已检测到卡但尚未读取。
    CardPresent,
    /// 读取成功。
    Done,
    /// 读取出错。
    Error(String),
}

/// 应用主状态。
pub struct App {
    /// PC/SC 管理器（可能初始化失败）。
    mgr: Option<PcscManager>,
    /// 读卡器名列表。
    pub readers: Vec<String>,
    /// 当前选中的读卡器索引。
    pub selected_reader: usize,
    /// 当前读卡器卡片状态。
    pub card_status: CardStatus,
    /// 读卡状态机。
    pub state: ReadState,
    /// 最近一次原始读卡结果。
    pub raw: Option<RawCardData>,
    /// 解析后的结果。
    pub parsed: Option<ParsedResult>,
    /// 当前焦点区。
    pub focus: Focus,
    /// 解析区滚动偏移。
    pub parsed_scroll: u16,
    /// APDU 区滚动偏移。
    pub apdu_scroll: u16,
    /// 是否请求退出。
    pub should_quit: bool,
    /// 顶部状态栏消息。
    pub message: String,
    /// 自动轮询开关。
    pub auto_poll: bool,
    /// 上次轮询时间。
    last_poll: Instant,
    /// 初始化错误（若 Pc/SC 不可用）。
    pub init_error: Option<String>,
    /// 旅行证件输入态（Some 表示正在录入 MRZ，此时暂停轮询与普通读卡）。
    pub traveldoc_input: Option<TravelDocInput>,
    /// 旅行证件输入当前聚焦字段。
    pub traveldoc_field: TravelDocField,
}

impl App {
    /// 构造并尝试初始化 PC/SC。
    pub fn new() -> Self {
        let (mgr, readers, init_error) = match PcscManager::new() {
            Ok(m) => {
                let readers = m.list_readers().unwrap_or_default();
                (Some(m), readers, None)
            }
            Err(e) => (None, Vec::new(), Some(e.to_string())),
        };
        let msg = if init_error.is_some() {
            "PC/SC 初始化失败，仅可查看界面".to_string()
        } else if readers.is_empty() {
            "未检测到读卡器".to_string()
        } else {
            "就绪：放卡或按 Enter 读取".to_string()
        };
        Self {
            mgr,
            readers,
            selected_reader: 0,
            card_status: CardStatus::Empty,
            state: ReadState::Idle,
            raw: None,
            parsed: None,
            focus: Focus::Readers,
            parsed_scroll: 0,
            apdu_scroll: 0,
            should_quit: false,
            message: msg,
            auto_poll: true,
            last_poll: Instant::now(),
            init_error,
            traveldoc_input: None,
            traveldoc_field: TravelDocField::DocNumber,
        }
    }

    /// 当前选中的读卡器名。
    pub fn current_reader(&self) -> Option<&str> {
        self.readers.get(self.selected_reader).map(|s| s.as_str())
    }

    /// APDU 历史（借用）。
    pub fn apdu_history(&self) -> &[ApduTrace] {
        self.raw.as_ref().map(|r| r.apdu_history.as_slice()).unwrap_or(&[])
    }

    /// 刷新读卡器列表（热插拔时）。
    pub fn refresh_readers(&mut self) {
        if let Some(mgr) = &self.mgr {
            if let Ok(list) = mgr.list_readers() {
                self.readers = list;
                if self.selected_reader >= self.readers.len() {
                    self.selected_reader = self.readers.len().saturating_sub(1);
                }
            }
        }
    }

    /// 周期性轮询卡片状态：检测放卡/取卡，自动触发读取。
    pub fn tick(&mut self) {
        if self.mgr.is_none() {
            return;
        }
        // 旅行证件输入进行中：暂停自动轮询，避免打断录入。
        if self.traveldoc_input.is_some() {
            return;
        }
        if self.last_poll.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.last_poll = Instant::now();

        if self.readers.is_empty() {
            self.refresh_readers();
            return;
        }

        let reader = match self.current_reader() {
            Some(r) => r.to_string(),
            None => return,
        };
        let status = self
            .mgr
            .as_ref()
            .and_then(|m| m.status(&reader).ok())
            .unwrap_or(CardStatus::Empty);

        let was_present = self.card_status == CardStatus::Present;
        self.card_status = status.clone();

        match status {
            CardStatus::Present => {
                // 放卡瞬间且开启自动轮询：自动读取一次。
                if !was_present && self.auto_poll {
                    self.read_now();
                } else if !was_present {
                    self.state = ReadState::CardPresent;
                    self.message = "检测到卡片，按 Enter 读取".to_string();
                }
            }
            CardStatus::Empty => {
                if was_present {
                    // 取卡：清空结果，回到空闲。
                    self.state = ReadState::Idle;
                    self.message = "卡片已取出".to_string();
                }
            }
        }
    }

    /// 立即读取当前读卡器一次并解析。
    pub fn read_now(&mut self) {
        let reader = match self.current_reader() {
            Some(r) => r.to_string(),
            None => {
                self.message = "无可用读卡器".to_string();
                return;
            }
        };
        let Some(mgr) = &self.mgr else {
            self.message = "PC/SC 不可用".to_string();
            return;
        };
        self.message = "读取中…".to_string();
        match read_from_reader(mgr, &reader) {
            Ok(data) => {
                let parsed = parse::parse(&data);
                self.parsed = Some(parsed);
                self.raw = Some(data);
                self.state = ReadState::Done;
                self.parsed_scroll = 0;
                self.apdu_scroll = 0;
                self.message = "读取完成".to_string();
            }
            Err(Error::NoCard) => {
                self.state = ReadState::Idle;
                self.message = "读卡器上无卡片".to_string();
            }
            Err(e) => {
                let msg = e.to_string();
                self.state = ReadState::Error(msg.clone());
                self.message = format!("读取错误: {msg}");
            }
        }
    }

    /// 切换焦点区（Tab）。
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Readers => Focus::Parsed,
            Focus::Parsed => Focus::Apdu,
            Focus::Apdu => Focus::Readers,
        };
    }

    /// 向上（依焦点作用于选择或滚动）。
    pub fn on_up(&mut self) {
        match self.focus {
            Focus::Readers => {
                if self.selected_reader > 0 {
                    self.selected_reader -= 1;
                    self.card_status = CardStatus::Empty;
                }
            }
            Focus::Parsed => self.parsed_scroll = self.parsed_scroll.saturating_sub(1),
            Focus::Apdu => self.apdu_scroll = self.apdu_scroll.saturating_sub(1),
        }
    }

    /// 向下。
    pub fn on_down(&mut self) {
        match self.focus {
            Focus::Readers => {
                if self.selected_reader + 1 < self.readers.len() {
                    self.selected_reader += 1;
                    self.card_status = CardStatus::Empty;
                }
            }
            Focus::Parsed => self.parsed_scroll = self.parsed_scroll.saturating_add(1),
            Focus::Apdu => self.apdu_scroll = self.apdu_scroll.saturating_add(1),
        }
    }

    /// 翻页（PageUp/Down）。
    pub fn on_page(&mut self, up: bool) {
        let delta = 10;
        let target = match self.focus {
            Focus::Parsed => &mut self.parsed_scroll,
            Focus::Apdu => &mut self.apdu_scroll,
            Focus::Readers => return,
        };
        *target = if up {
            target.saturating_sub(delta)
        } else {
            target.saturating_add(delta)
        };
    }

    /// 切换自动轮询。
    pub fn toggle_auto(&mut self) {
        self.auto_poll = !self.auto_poll;
        self.message = if self.auto_poll {
            "自动读卡：开".to_string()
        } else {
            "自动读卡：关".to_string()
        };
    }

    /// 是否正处于旅行证件输入态。
    pub fn is_traveldoc_input(&self) -> bool {
        self.traveldoc_input.is_some()
    }

    /// 进入旅行证件 MRZ 输入态。
    pub fn start_traveldoc_input(&mut self) {
        self.traveldoc_input = Some(TravelDocInput::default());
        self.traveldoc_field = TravelDocField::DocNumber;
        self.message = "旅行证件模式：输入 MRZ 三要素，Enter 读取，Esc 取消".to_string();
    }

    /// 取消旅行证件输入，回到普通模式。
    pub fn cancel_traveldoc_input(&mut self) {
        self.traveldoc_input = None;
        self.message = "已退出旅行证件模式".to_string();
    }

    /// 旅行证件输入：向当前字段追加一个字符。
    pub fn traveldoc_push_char(&mut self, c: char) {
        let field = self.traveldoc_field;
        if let Some(input) = &mut self.traveldoc_input {
            input.field_mut(field).push(c.to_ascii_uppercase());
        }
    }

    /// 旅行证件输入：删除当前字段末尾字符。
    pub fn traveldoc_backspace(&mut self) {
        let field = self.traveldoc_field;
        if let Some(input) = &mut self.traveldoc_input {
            input.field_mut(field).pop();
        }
    }

    /// 旅行证件输入：切换到下一个字段（Tab / Down）。
    pub fn traveldoc_next_field(&mut self) {
        self.traveldoc_field = match self.traveldoc_field {
            TravelDocField::DocNumber => TravelDocField::Dob,
            TravelDocField::Dob => TravelDocField::Doe,
            TravelDocField::Doe => TravelDocField::DocNumber,
        };
    }

    /// 旅行证件输入：切换到上一个字段（Up）。
    pub fn traveldoc_prev_field(&mut self) {
        self.traveldoc_field = match self.traveldoc_field {
            TravelDocField::DocNumber => TravelDocField::Doe,
            TravelDocField::Dob => TravelDocField::DocNumber,
            TravelDocField::Doe => TravelDocField::Dob,
        };
    }

    /// 用当前输入的 MRZ 三要素读取旅行证件并解析。
    pub fn read_traveldoc_now(&mut self) {
        let Some(input) = self.traveldoc_input.clone() else {
            return;
        };
        let key = PassportKey::new(
            input.doc_number.clone(),
            input.dob.clone(),
            input.doe.clone(),
        );
        if let Err(e) = key.validate() {
            self.message = format!("MRZ 输入无效: {e}");
            return;
        }
        let reader = match self.current_reader() {
            Some(r) => r.to_string(),
            None => {
                self.message = "无可用读卡器".to_string();
                return;
            }
        };
        let Some(mgr) = &self.mgr else {
            self.message = "PC/SC 不可用".to_string();
            return;
        };
        self.message = "读取旅行证件中…（约需数秒）".to_string();
        match read_traveldoc_from_reader(mgr, &reader, &key) {
            Ok(data) => {
                let parsed = parse::parse(&data);
                self.parsed = Some(parsed);
                self.raw = Some(data);
                self.state = ReadState::Done;
                self.parsed_scroll = 0;
                self.apdu_scroll = 0;
                self.traveldoc_input = None;
                // 同步卡片状态为在位，避免 tick 恢复轮询后误判为“新放卡”
                // 而触发普通 read_now() 覆盖掉旅行证件结果。
                self.card_status = CardStatus::Present;
                self.message = "旅行证件读取完成".to_string();
            }
            Err(e) => {
                let msg = e.to_string();
                self.state = ReadState::Error(msg.clone());
                self.message = format!("旅行证件读取失败: {msg}");
            }
        }
    }
}

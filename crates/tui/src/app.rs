//! TUI 应用状态：读卡器管理、卡片状态轮询、读卡结果与解析、滚动/焦点。

use crate::history::{self, SavedCard};
use crate::parse::{self, ParsedResult};
use crate::settings::{self, Settings};
use std::time::{Duration, Instant};
use tarot_backend::reader::{CardStatus, PcscManager};
use tarot_backend::{read_from_reader, read_traveldoc_from_reader};
use tarot_core::{Error, PassportKey, RawCardData};

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
    Saved,
}

/// 设置菜单条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsItem {
    AutoPoll,
    AutoSave,
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
    /// 已保存卡片列表滚动偏移。
    pub saved_scroll: u16,
    /// 当前选中的已保存卡片索引。
    pub selected_saved: usize,
    /// 已保存的交通卡历史。
    pub saved_cards: Vec<SavedCard>,
    /// 当前展示的历史卡片索引。
    pub history_view: Option<usize>,
    /// 是否请求退出。
    pub should_quit: bool,
    /// 顶部状态栏消息。
    pub message: String,
    /// 自动轮询开关。
    pub auto_poll: bool,
    /// 读取交通卡后自动保存记录。
    pub auto_save: bool,
    /// 设置菜单是否打开。
    pub settings_open: bool,
    /// 设置菜单当前选中项。
    pub selected_setting: SettingsItem,
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
        let settings = settings::load();
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
            saved_scroll: 0,
            selected_saved: 0,
            saved_cards: history::load(),
            history_view: None,
            should_quit: false,
            message: msg,
            auto_poll: settings.auto_poll,
            auto_save: settings.auto_save,
            settings_open: false,
            selected_setting: SettingsItem::AutoPoll,
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
                let has_transit = history::transport_cards(&parsed).next().is_some();
                self.parsed = Some(parsed);
                self.raw = Some(data);
                self.state = ReadState::Done;
                self.parsed_scroll = 0;
                self.history_view = None;
                self.message = if has_transit && self.auto_save {
                    self.save_current_transit();
                    if self.message.starts_with("已保存") {
                        format!("读取完成：{}", self.message)
                    } else {
                        self.message.clone()
                    }
                } else if has_transit {
                    "读取完成：按 s 保存交通卡记录".to_string()
                } else {
                    "读取完成".to_string()
                };
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
            Focus::Parsed => Focus::Saved,
            Focus::Saved => Focus::Readers,
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
            Focus::Saved => {
                if self.selected_saved > 0 {
                    self.selected_saved -= 1;
                    self.saved_scroll = self.saved_scroll.saturating_sub(1);
                }
            }
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
            Focus::Saved => {
                if self.selected_saved + 1 < self.saved_cards.len() {
                    self.selected_saved += 1;
                    self.saved_scroll = self.saved_scroll.saturating_add(1);
                }
            }
        }
    }

    /// 翻页（PageUp/Down）。
    pub fn on_page(&mut self, up: bool) {
        let delta = 10;
        let target = match self.focus {
            Focus::Parsed => &mut self.parsed_scroll,
            Focus::Saved => &mut self.saved_scroll,
            Focus::Readers => return,
        };
        *target = if up {
            target.saturating_sub(delta)
        } else {
            target.saturating_add(delta)
        };
    }

    /// Enter：保存卡片列表聚焦时查看历史，否则立即读卡。
    pub fn on_enter(&mut self) {
        if self.focus == Focus::Saved {
            self.open_selected_saved();
        } else {
            self.read_now();
        }
    }

    /// 保存当前读取结果中的所有交通卡。
    pub fn save_current_transit(&mut self) {
        let Some(parsed) = &self.parsed else {
            self.message = "尚无可保存的读卡结果".to_string();
            return;
        };

        let mut transit_cards = 0usize;
        let mut new_records = 0usize;
        for card in history::transport_cards(parsed) {
            transit_cards += 1;
            let mut snapshot = history::snapshot(card);
            let record = snapshot.records.pop();
            let Some(record) = record else {
                continue;
            };
            if let Some(idx) = self.saved_cards.iter().position(|c| c.key == snapshot.key) {
                let existing = &mut self.saved_cards[idx];
                existing.name = snapshot.name;
                existing.number = snapshot.number;
                existing.currency = snapshot.currency;
                new_records += history::append_new_transactions(existing, card);
                self.selected_saved = idx;
            } else {
                snapshot.records.push(record);
                self.saved_cards.push(snapshot);
                self.selected_saved = self.saved_cards.len().saturating_sub(1);
                new_records += card.transactions.len().max(1);
            }
        }

        if transit_cards == 0 {
            self.message = "当前卡片不是可保存的交通卡".to_string();
            return;
        }
        if new_records == 0 {
            self.message = "没有新的交易记录".to_string();
            return;
        }
        match history::save(&self.saved_cards) {
            Ok(()) => self.message = format!("已保存 {new_records} 条新记录"),
            Err(e) => self.message = format!("保存失败: {e}"),
        }
    }

    /// 打开右侧选中的已保存卡片历史。
    pub fn open_selected_saved(&mut self) {
        if self.saved_cards.is_empty() {
            self.message = "暂无已保存卡片".to_string();
            return;
        }
        self.selected_saved = self.selected_saved.min(self.saved_cards.len() - 1);
        self.history_view = Some(self.selected_saved);
        self.parsed_scroll = 0;
        self.message = format!(
            "已打开 {} 的历史记录",
            self.saved_cards[self.selected_saved].display_name()
        );
    }

    /// 切换自动轮询。
    pub fn toggle_auto(&mut self) {
        self.auto_poll = !self.auto_poll;
        self.persist_settings();
        self.message = if self.auto_poll {
            "自动读卡：开".to_string()
        } else {
            "自动读卡：关".to_string()
        };
    }

    /// 切换自动保存交通卡记录。
    pub fn toggle_auto_save(&mut self) {
        self.auto_save = !self.auto_save;
        self.persist_settings();
        self.message = if self.auto_save {
            "自动保存：开".to_string()
        } else {
            "自动保存：关".to_string()
        };
    }

    /// 设置菜单是否打开。
    pub fn is_settings_open(&self) -> bool {
        self.settings_open
    }

    /// 打开设置菜单。
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.message = "设置：↑↓ 选择，Enter/Space 切换，Esc 关闭".to_string();
    }

    /// 关闭设置菜单。
    pub fn close_settings(&mut self) {
        self.settings_open = false;
        self.message = "已关闭设置".to_string();
    }

    /// 设置菜单选择上一项。
    pub fn settings_prev_item(&mut self) {
        self.selected_setting = match self.selected_setting {
            SettingsItem::AutoPoll => SettingsItem::AutoSave,
            SettingsItem::AutoSave => SettingsItem::AutoPoll,
        };
    }

    /// 设置菜单选择下一项。
    pub fn settings_next_item(&mut self) {
        self.settings_prev_item();
    }

    /// 切换设置菜单当前项。
    pub fn toggle_selected_setting(&mut self) {
        match self.selected_setting {
            SettingsItem::AutoPoll => self.toggle_auto(),
            SettingsItem::AutoSave => self.toggle_auto_save(),
        }
    }

    fn persist_settings(&mut self) {
        let settings = Settings {
            auto_poll: self.auto_poll,
            auto_save: self.auto_save,
        };
        if let Err(e) = settings::save(&settings) {
            self.message = format!("保存设置失败: {e}");
        }
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
                self.history_view = None;
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

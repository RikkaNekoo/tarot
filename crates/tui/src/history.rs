//! Local saved transit card history for the TUI.

use crate::parse::{ParsedCard, ParsedResult};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const HEADER: &str = "tarot-tui-history-v1";
const TRANSIT_NAMES: &[&str] = &[
    "深圳通",
    "武汉通",
    "岭南通",
    "城市一卡通",
    "交通联合",
    "澳门通",
    "北京一卡通",
    "T-Money",
    "八达通 Octopus",
];

#[derive(Debug, Clone, Default)]
pub struct SavedRecord {
    pub timestamp: u64,
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SavedCard {
    pub key: String,
    pub name: String,
    pub number: String,
    pub currency: String,
    pub records: Vec<SavedRecord>,
}

impl SavedCard {
    pub fn display_name(&self) -> String {
        if self.number.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.number)
        }
    }
}

pub fn load() -> Vec<SavedCard> {
    let Ok(text) = fs::read_to_string(path()) else {
        return Vec::new();
    };
    let mut cards: Vec<SavedCard> = Vec::new();
    let mut current: Option<SavedCard> = None;

    for line in text.lines() {
        if line == HEADER || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.as_slice() {
            ["card", key, name, number, currency] => {
                if let Some(card) = current.take() {
                    cards.push(card);
                }
                current = Some(SavedCard {
                    key: decode(key),
                    name: decode(name),
                    number: decode(number),
                    currency: decode(currency),
                    records: Vec::new(),
                });
            }
            ["record", ts, title, lines] => {
                if let Some(card) = &mut current {
                    let timestamp = ts.parse::<u64>().unwrap_or_default();
                    let body = decode(lines);
                    card.records.push(SavedRecord {
                        timestamp,
                        title: decode(title),
                        lines: body.lines().map(str::to_string).collect(),
                    });
                }
            }
            _ => {}
        }
    }
    if let Some(card) = current {
        cards.push(card);
    }
    cards
}

pub fn save(cards: &[SavedCard]) -> std::io::Result<()> {
    let file = path();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    for card in cards {
        out.push_str(&format!(
            "card\t{}\t{}\t{}\t{}\n",
            encode(&card.key),
            encode(&card.name),
            encode(&card.number),
            encode(&card.currency)
        ));
        for record in &card.records {
            out.push_str(&format!(
                "record\t{}\t{}\t{}\n",
                record.timestamp,
                encode(&record.title),
                encode(&record.lines.join("\n"))
            ));
        }
    }
    fs::write(file, out)
}

pub fn transport_cards(parsed: &ParsedResult) -> impl Iterator<Item = &ParsedCard> {
    parsed.cards.iter().filter(|card| is_transport(card))
}

pub fn is_transport(card: &ParsedCard) -> bool {
    TRANSIT_NAMES.contains(&card.name.as_str()) || !card.protocols.is_empty()
}

pub fn snapshot(card: &ParsedCard) -> SavedCard {
    let number = card.number.clone().unwrap_or_default();
    let key = if number.is_empty() {
        card.name.clone()
    } else {
        format!("{}:{number}", card.name)
    };
    SavedCard {
        key,
        name: card.name.clone(),
        number,
        currency: card.currency.clone(),
        records: vec![SavedRecord {
            timestamp: now_secs(),
            title: record_title(card),
            lines: record_lines(card),
        }],
    }
}

fn record_title(card: &ParsedCard) -> String {
    if let Some(balance) = card.balance {
        return format!("余额 {}{balance:.2}", card.currency);
    }
    if let Some(protocol) = card.protocols.iter().find(|p| p.name == "交通联合") {
        if let Some(balance) = protocol.balance {
            return format!("余额 {}{balance:.2}", protocol.currency);
        }
    }
    format!("{} 条记录", card.transactions.len())
}

fn record_lines(card: &ParsedCard) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("卡种: {}", card.name));
    if let Some(number) = &card.number {
        lines.push(format!("卡号: {number}"));
    }
    if let Some(balance) = card.balance {
        lines.push(format!("余额: {}{balance:.2}", card.currency));
    }
    for (k, v) in &card.fields {
        lines.push(format!("{k}: {v}"));
    }
    for protocol in &card.protocols {
        if let Some(number) = &protocol.number {
            lines.push(format!("{}卡号: {number}", protocol.name));
        }
        if protocol.name == "交通联合" {
            if let Some(balance) = protocol.balance {
                lines.push(format!("余额: {}{balance:.2}", protocol.currency));
            }
        }
        for (k, v) in &protocol.fields {
            lines.push(format!("{k}: {v}"));
        }
        for note in &protocol.notes {
            lines.push(format!("提示: {note}"));
        }
    }
    if !card.transactions.is_empty() {
        lines.push("记录:".to_string());
        for t in &card.transactions {
            let seq = t.seq.map(|s| format!("#{s} ")).unwrap_or_default();
            let dt = match (t.date.is_empty(), t.time.is_empty()) {
                (true, true) => String::new(),
                (false, true) => t.date.clone(),
                (true, false) => t.time.clone(),
                (false, false) => format!("{} {}", t.date, t.time),
            };
            let kind = if t.trip_kind.is_empty() {
                t.kind.clone()
            } else {
                format!("{} [{}]", t.trip_kind, t.aux)
            };
            let mut line = format!("  {seq}{kind} {:+.2}{}", t.amount, card.currency);
            if let Some(balance) = t.balance_after {
                line.push_str(&format!(" 余{balance:.2}{}", card.currency));
            }
            if !t.source.is_empty() {
                line.push_str(&format!(" [{}]", t.source));
            }
            if !dt.is_empty() {
                line.push_str(&format!(" {dt}"));
            }
            lines.push(line);
        }
    }
    for note in &card.notes {
        lines.push(format!("提示: {note}"));
    }
    lines
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/tarot/tui-history.tsv");
    }
    PathBuf::from("tarot-tui-history.tsv")
}

fn encode(s: &str) -> String {
    hex::encode(s.as_bytes())
}

fn decode(s: &str) -> String {
    hex::decode(s)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default()
}

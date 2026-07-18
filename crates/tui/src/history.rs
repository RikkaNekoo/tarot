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

pub fn append_new_transactions(existing: &mut SavedCard, card: &ParsedCard) -> usize {
    let new_lines: Vec<String> = transaction_lines(card)
        .into_iter()
        .filter(|line| !has_transaction_line(existing, line))
        .collect();
    if new_lines.is_empty() {
        return 0;
    }

    let count = new_lines.len();
    append_transaction_lines(existing, new_lines);
    count
}

fn record_title(card: &ParsedCard) -> String {
    format!("{} 条记录", card.transactions.len())
}

fn record_lines(card: &ParsedCard) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("卡种: {}", card.name));
    if let Some(number) = &card.number {
        lines.push(format!("卡号: {number}"));
    }
    for (k, v) in &card.fields {
        if k == "广州优惠金额累计" {
            continue;
        }
        lines.push(format!("{k}: {v}"));
    }
    for protocol in &card.protocols {
        if protocol.name != "交通联合" {
            if let Some(number) = &protocol.number {
                lines.push(format!("{}卡号: {number}", protocol.name));
            }
        }
        for (k, v) in &protocol.fields {
            if k == "广州优惠金额累计" {
                continue;
            }
            lines.push(format!("{k}: {v}"));
        }
        for note in &protocol.notes {
            lines.push(format!("提示: {note}"));
        }
    }
    if !card.transactions.is_empty() {
        lines.push("记录:".to_string());
        lines.extend(transaction_lines(card));
    }
    for note in &card.notes {
        lines.push(format!("提示: {note}"));
    }
    lines
}

fn transaction_lines(card: &ParsedCard) -> Vec<String> {
    card.transactions
        .iter()
        .map(|t| {
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
            let mut line = format!("  {kind} {:+.2}{}", t.amount, card.currency);
            if !t.source.is_empty() {
                line.push_str(&format!(" [{}]", t.source));
            }
            if !dt.is_empty() {
                line.push_str(&format!(" {dt}"));
            }
            line
        })
        .collect()
}

fn has_transaction_line(card: &SavedCard, line: &str) -> bool {
    card.records
        .iter()
        .any(|record| record.lines.iter().any(|saved| saved == line))
}

fn append_transaction_lines(card: &mut SavedCard, lines: Vec<String>) {
    merge_records(card);

    if card.records.is_empty() {
        card.records.push(SavedRecord {
            timestamp: now_secs(),
            title: "0 条记录".to_string(),
            lines: vec!["记录:".to_string()],
        });
    }

    let record = &mut card.records[0];
    record.timestamp = now_secs();
    if !record.lines.iter().any(|line| line == "记录:") {
        record.lines.push("记录:".to_string());
    }
    let insert_at = record
        .lines
        .iter()
        .position(|line| line == "记录:")
        .map(|index| index + 1)
        .unwrap_or(record.lines.len());
    record.lines.splice(insert_at..insert_at, lines);
    record.title = format!("{} 条记录", transaction_line_count(record));
}

fn merge_records(card: &mut SavedCard) {
    if card.records.len() <= 1 {
        return;
    }

    let mut merged = card.records.pop().unwrap_or_default();
    if !merged.lines.iter().any(|line| line == "记录:") {
        merged.lines.push("记录:".to_string());
    }

    let mut lines = Vec::new();
    for record in card.records.drain(..) {
        for line in record.lines {
            if line.starts_with("  ") && !merged.lines.iter().any(|saved| saved == &line) {
                lines.push(line);
            }
        }
    }
    card.records.push(merged);
    append_transaction_lines(card, lines);
}

fn transaction_line_count(record: &SavedRecord) -> usize {
    record
        .lines
        .iter()
        .filter(|line| line.starts_with("  "))
        .count()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::model::Transaction;

    #[test]
    fn append_new_transactions_only_appends_unseen_transactions() {
        let mut first = ParsedCard::new("交通联合");
        first.number = Some("31000001".to_string());
        first.transactions = vec![tx(1, "2026-07-09"), tx(2, "2026-07-08")];
        let mut saved = snapshot(&first);

        let mut next = ParsedCard::new("交通联合");
        next.number = Some("31000001".to_string());
        next.transactions = vec![tx(3, "2026-07-10"), tx(1, "2026-07-09")];

        assert_eq!(append_new_transactions(&mut saved, &next), 1);
        assert_eq!(saved.records.len(), 1);
        assert_eq!(saved.records[0].title, "3 条记录");
        assert_eq!(transaction_line_count(&saved.records[0]), 3);
        let marker = saved.records[0]
            .lines
            .iter()
            .position(|line| line == "记录:")
            .unwrap();
        assert!(saved.records[0].lines[marker + 1].contains("消费 -2.00¥ 2026-07-10"));
        assert!(saved.records[0].lines[marker + 2].contains("消费 -2.00¥ 2026-07-09"));
    }

    #[test]
    fn history_snapshot_omits_balance_and_monthly_discount() {
        let mut card = ParsedCard::new("岭南通");
        card.balance = Some(42.0);
        card.add_field("广州优惠金额累计", "2026-07 ¥3.00");
        card.transactions = vec![tx(1, "2026-07-09")];

        let saved = snapshot(&card);
        let text = saved.records[0].lines.join("\n");

        assert!(!text.contains("余额:"));
        assert!(!text.contains("广州优惠金额累计"));
    }

    fn tx(seq: u64, date: &str) -> Transaction {
        Transaction {
            seq: Some(seq),
            kind: "消费".to_string(),
            amount: -2.0,
            date: date.to_string(),
            time: "08:00:00".to_string(),
            ..Default::default()
        }
    }
}

//! T-Money（韩国）解析。purse info 内嵌于 SELECT FCI（`tmoney_fci`，跳过前 4 字节），
//! 余额来自 `TMoney_balance`，记录来自 `TMoney_trans_N`（长度 0x2E）。

use super::model::{ParsedCard, Transaction};
use super::util::*;
use tarot_core::RawCardData;

fn slice(hex: &str, start: usize, end: usize) -> &str {
    if start >= hex.len() {
        return "";
    }
    &hex[start..end.min(hex.len())]
}

pub fn parse(raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new("T-Money");
    card.currency = "₩".to_string();

    // 余额：专有 4 字节大端。
    if let Some(hex) = raw.get("TMoney_balance") {
        let bytes = hex_to_bytes(hex);
        if bytes.len() >= 4 {
            // 韩元无小数。
            card.balance = Some(be_uint(&bytes, 0, 4) as f64);
        }
    }

    // purse info：FCI 跳过前 4 字节（= hex 前 8 位）。
    if let Some(fci) = raw.get("tmoney_fci") {
        let purse = if fci.len() > 8 { &fci[8..] } else { fci };
        // 卡号 24..34（相对 purse 的 hex 位）
        let num = slice(purse, 24, 34);
        if !num.is_empty() {
            card.number = Some(trim_leading_zeros(num));
        }
        let issue = fmt_date8(slice(purse, 34, 42));
        if !issue.is_empty() {
            card.add_field("发行日期", issue);
        }
        let expire = fmt_date8(slice(purse, 42, 50));
        if !expire.is_empty() {
            card.add_field("有效期至", expire);
        }
    }

    // 交易记录：类型 01=消费 02=充值。布局与 PBOC 不同，尽力解析金额与类型。
    for n in 1u8..=10 {
        let key = format!("TMoney_trans_{n}");
        let Some(hex) = raw.get(&key) else {
            continue;
        };
        if hex.chars().all(|c| c == '0') {
            continue;
        }
        let bytes = hex_to_bytes(hex);
        if bytes.len() < 12 {
            continue;
        }
        let mut t = Transaction::default();
        // 记录类型在偏移 0 字节（尽力）。
        let kind = slice(hex, 0, 2);
        t.kind = match kind {
            "01" => "消费".to_string(),
            "02" => "充值".to_string(),
            _ => "其他".to_string(),
        };
        // 金额：取 4 字节大端（偏移字节 1..5，尽力估计）。
        let amount = be_uint(&bytes, 1, 5);
        t.amount = amount as f64;
        if kind == "01" {
            t.amount = -t.amount;
        }
        card.transactions.push(t);
    }

    if card.balance.is_none() && card.number.is_none() {
        card.notes.push("T-Money 关键字段缺失".into());
    }

    card
}
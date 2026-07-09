//! 前端解析层：把后端抓到的 `RawCardData`（原始 hex）按 card_type / sub_cards
//! 分派到各卡种解析器，产出人类可读的 `ParsedResult`。

pub mod codes;
pub mod emv;
pub mod felica;
pub mod mifare;
pub mod model;
pub mod tlv;
pub mod tmoney;
pub mod transit;
pub mod traveldoc;
pub mod util;

pub use model::{ParsedCard, ParsedResult};

use tarot_core::RawCardData;

use self::model::{ProtocolSection, Transaction};

/// 主入口：解析整张卡（含叠加子卡）。
pub fn parse(raw: &RawCardData) -> ParsedResult {
    let mut result = ParsedResult {
        card_type: raw.card_type.clone(),
        atr: raw.atr.clone(),
        cards: Vec::new(),
    };

    // 单一物理层卡（Mifare/Felica）直接按 card_type 分派。
    match raw.card_type.as_str() {
        "MifareUltralight" => {
            result.cards.push(mifare::parse_ultralight(raw));
            return result;
        }
        "MifareClassic" => {
            result.cards.push(mifare::parse_classic(raw));
            return result;
        }
        "Octopus" | "Felica" => {
            result.cards.push(felica::parse(raw));
            return result;
        }
        "TravelDoc" => {
            result.cards.push(traveldoc::parse(raw));
            return result;
        }
        _ => {}
    }

    // Type A / B：按 sub_cards 逐一解析。
    for sub in &raw.sub_cards {
        let card = parse_sub(sub, raw);
        result.cards.push(card);
    }

    fold_multi_transit_cards(&mut result.cards);

    // 未识别到任何子卡但 card_type 有内容：给出占位卡。
    if result.cards.is_empty() {
        let mut c = ParsedCard::new(if raw.card_type == "Unknown" {
            "未识别卡片".to_string()
        } else {
            raw.card_type.clone()
        });
        if raw.raw_fields.is_empty() {
            c.notes.push("未抓到任何数据".into());
        } else {
            c.notes.push("有原始数据但无匹配解析器".into());
        }
        result.cards.push(c);
    }

    result
}

fn fold_multi_transit_cards(cards: &mut Vec<ParsedCard>) {
    let transit_indices: Vec<usize> = cards
        .iter()
        .enumerate()
        .filter_map(|(idx, card)| is_transit_card(card).then_some(idx))
        .collect();
    if transit_indices.len() < 2 {
        return;
    }

    let insert_at = transit_indices[0];
    let mut transit_cards = Vec::new();
    for idx in transit_indices.into_iter().rev() {
        transit_cards.push(cards.remove(idx));
    }
    transit_cards.reverse();

    let mut combined = ParsedCard::new(
        transit_cards
            .iter()
            .map(|card| card.name.as_str())
            .collect::<Vec<_>>()
            .join("+"),
    );
    combined.number = transit_cards
        .iter()
        .find(|card| card.name == "交通联合")
        .and_then(|card| card.number.clone())
        .or_else(|| transit_cards.iter().find_map(|card| card.number.clone()));
    combined.currency = transit_cards
        .first()
        .map(|card| card.currency.clone())
        .unwrap_or_else(|| "¥".to_string());

    for card in transit_cards {
        let source = card.name.clone();
        combined.protocols.push(ProtocolSection {
            name: card.name,
            number: card.number,
            balance: card.balance,
            currency: card.currency,
            fields: card.fields,
            notes: card.notes,
        });
        combined
            .transactions
            .extend(card.transactions.into_iter().map(|mut tx| {
                tx.source = source.clone();
                tx
            }));
    }
    complete_lingnan_transaction_years(&mut combined.transactions);
    combined.transactions.sort_by(|a, b| {
        let a_key = transaction_sort_key(a);
        let b_key = transaction_sort_key(b);
        b_key.cmp(&a_key)
    });

    cards.insert(insert_at, combined);
}

fn complete_lingnan_transaction_years(transactions: &mut [Transaction]) {
    let anchors: Vec<i32> = transactions
        .iter()
        .filter(|tx| tx.source == "交通联合")
        .filter_map(|tx| transaction_sort_key(tx))
        .collect();
    if anchors.is_empty() {
        return;
    }

    for tx in transactions.iter_mut().filter(|tx| tx.source == "岭南通") {
        let Some((month, day)) = parse_month_day(&tx.date) else {
            continue;
        };
        let Some(year) = nearest_year_for_month_day(month, day, &anchors) else {
            continue;
        };
        tx.date = format!("{year:04}-{month:02}-{day:02}");
    }
}

fn nearest_year_for_month_day(month: u32, day: u32, anchors: &[i32]) -> Option<i32> {
    anchors
        .iter()
        .flat_map(|anchor| {
            let year = civil_from_days(anchor / 86_400).0;
            [year - 1, year, year + 1]
        })
        .filter_map(|year| timestamp_for_date_time(year, month, day, 0, 0, 0).map(|ts| (year, ts)))
        .min_by_key(|(_, ts)| {
            anchors
                .iter()
                .map(|anchor| (ts - anchor).abs())
                .min()
                .unwrap_or(0)
        })
        .map(|(year, _)| year)
}

fn is_transit_card(card: &ParsedCard) -> bool {
    matches!(
        card.name.as_str(),
        "深圳通"
            | "武汉通"
            | "岭南通"
            | "城市一卡通"
            | "交通联合"
            | "澳门通"
            | "北京一卡通"
            | "T-Money"
    )
}

fn transaction_sort_key(tx: &Transaction) -> Option<i32> {
    let (year, month, day) = parse_full_date(&tx.date)?;
    let (hour, minute, second) = parse_time(&tx.time).unwrap_or((0, 0, 0));
    timestamp_for_date_time(year, month, day, hour, minute, second)
}

fn parse_full_date(date: &str) -> Option<(i32, u32, u32)> {
    let mut parts = date.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !valid_month_day(month, day) {
        return None;
    }
    Some((year, month, day))
}

fn parse_month_day(date: &str) -> Option<(u32, u32)> {
    let mut parts = date.split('-');
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !valid_month_day(month, day) {
        return None;
    }
    Some((month, day))
}

fn parse_time(time: &str) -> Option<(u32, u32, u32)> {
    let mut parts = time.split(':');
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second))
}

fn valid_month_day(month: u32, day: u32) -> bool {
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn timestamp_for_date_time(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<i32> {
    if !valid_month_day(month, day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour as i32 * 3_600 + minute as i32 * 60 + second as i32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i32> {
    let max_day = days_in_month(year, month)?;
    if day == 0 || day > max_day {
        return None;
    }
    let year = year - (month <= 2) as i32;
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + (month <= 2) as i32, month as u32, day as u32)
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        _ => None,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 分派单张子卡到对应解析器。
fn parse_sub(sub: &str, raw: &RawCardData) -> ParsedCard {
    match sub {
        "EMV" => emv::parse(raw),
        "Octopus" => felica::parse(raw),
        "MifareDESFire" => mifare::parse_desfire(raw),
        "TMoney" => tmoney::parse(raw),
        "ShenzhenTong" | "WuhanTong" | "LingnanPass" | "CityUnion" | "TUnion" | "MacauPass"
        | "BMAC" | "MotBmac" | "ChinaTransit" | "SuXin" | "SzpkZyy" => transit::parse(sub, raw),
        other => {
            // EMV 品牌型 card_type（如 "EMV:Visa"）也归 EMV。
            if raw.card_type.starts_with("EMV") {
                emv::parse(raw)
            } else {
                let mut c = ParsedCard::new(other.to_string());
                c.notes.push("无专用解析器".into());
                c
            }
        }
    }
}

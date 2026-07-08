//! PBOC 交通卡解析：深圳通/武汉通/岭南通/City Union/交通联合/澳门通/北京 BMAC/T-Money。
//!
//! 文档 `docs/nfsee-apdu-analysis.md` 的偏移均以「hex 位」（十六进制字符位置）计。
//! 本模块直接在 hex 字符串上按字符切片，保持与文档一致。

use super::codes;
use super::model::{ParsedCard, Transaction, Trip};
use super::util::*;
use tarot_core::RawCardData;

/// 按 hex 字符位置切片（越界安全）。
fn slice(hex: &str, start: usize, end: usize) -> &str {
    if start >= hex.len() {
        return "";
    }
    &hex[start..end.min(hex.len())]
}

/// 交通卡子卡名 -> 中文可读名。
pub fn display_name(name: &str) -> &'static str {
    match name {
        "ShenzhenTong" => "深圳通",
        "WuhanTong" => "武汉通",
        "LingnanPass" => "岭南通",
        "CityUnion" => "城市一卡通",
        "TUnion" => "交通联合",
        "MacauPass" => "澳门通",
        "BMAC" => "北京一卡通",
        "TMoney" => "T-Money",
        _ => "交通卡",
    }
}

/// 解析一张 PBOC 交通子卡。`name` 为后端 sub_card 名。
pub fn parse(name: &str, raw: &RawCardData) -> ParsedCard {
    let mut card = ParsedCard::new(display_name(name));

    if name == "MacauPass" {
        card.currency = "MOP".to_string();
    }

    // 卡号 / 有效期：从 file15（或 BMAC file04）按各卡偏移解析。
    parse_card_number(name, raw, &mut card);

    // 余额。
    parse_balance(name, raw, &mut card);

    // 交易记录。
    parse_transactions(name, raw, &mut card);

    // 交通联合额外的省市/类型（17 号文件）。
    if name == "TUnion" {
        if let Some(f17) = raw.get("TUnion_file17") {
            let province = slice(f17, 8, 12);
            let city = slice(f17, 12, 16);
            let ctype = slice(f17, 20, 22);
            // 省/市查银联地区码表，未命中则显示原码。
            if !province.is_empty() {
                let v = codes::unionpay_region(province)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("({province})"));
                card.add_field("省份", v);
            }
            if !city.is_empty() {
                let v = codes::unionpay_region(city)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("({city})"));
                card.add_field("城市", v);
            }
            // 卡类型码为 16 进制字节，查 TUnionDF11Type 表。
            if let Ok(code) = u8::from_str_radix(ctype, 16) {
                let v = codes::tunion_df11_type(code)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("({ctype})"));
                card.add_field("卡类型", v);
            }
        }
        // SFI 0x1E 行程记录：与交易记录一一合并（同一次交易的两份视图）。
        let trips = parse_trips(raw);
        merge_trips_into_transactions(&mut card, trips);
    }

    card
}

/// 把行程记录按顺序合并进交易记录：第 i 条行程补充第 i 条交易的
/// 进出站/辅助类型/交易后余额/线路站点，并用行程时间戳补齐缺失的日期时间。
/// 若行程数多于交易数，多出的行程作为独立记录追加。
fn merge_trips_into_transactions(card: &mut ParsedCard, trips: Vec<Trip>) {
    for (i, trip) in trips.into_iter().enumerate() {
        if let Some(t) = card.transactions.get_mut(i) {
            t.trip_kind = trip.kind;
            t.aux = trip.aux;
            t.balance_after = Some(trip.balance);
            t.line_station = trip.line_station;
            // 交易记录日期时间为空（非 BCD）时，用行程时间戳拆出的日期时间补齐。
            if (t.date.is_empty() || t.time.is_empty()) && !trip.timestamp.is_empty() {
                if let Some((d, tm)) = trip.timestamp.split_once(' ') {
                    if t.date.is_empty() {
                        t.date = d.to_string();
                    }
                    if t.time.is_empty() {
                        t.time = tm.to_string();
                    }
                }
            }
        } else {
            // 没有对应交易记录：把行程本身转成一条交易记录。
            let (d, tm) = trip
                .timestamp
                .split_once(' ')
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .unwrap_or_default();
            card.transactions.push(Transaction {
                seq: None,
                kind: "消费".to_string(),
                amount: -trip.amount,
                date: d,
                time: tm,
                terminal: String::new(),
                aux: trip.aux,
                trip_kind: trip.kind,
                balance_after: Some(trip.balance),
                line_station: trip.line_station,
            });
        }
    }
}

/// 行程交易类型码 -> 可读名。
fn trip_kind(code: &str) -> &'static str {
    match code {
        "02" | "06" => "单次",
        "03" => "进站",
        "04" => "出站",
        _ => "其他",
    }
}

/// 行程辅助类型码 -> 可读名。
fn trip_aux(code: &str) -> &'static str {
    match code {
        "01" => "地铁",
        "02" => "公交",
        _ => "其他",
    }
}

/// 解析交通联合行程记录（SFI 0x1E，每条 0x30 字节）。
///
/// 字节偏移（对照用户提供的规范）：
/// - 0x00 交易类型(1 BCD)
/// - 0x09 辅助类型(1 BCD)
/// - 0x0A 线路和站点(7 BCD)
/// - 0x11 交易金额(4 BIN 大端，分)
/// - 0x15 余额(4 BIN 大端，分)
/// - 0x19 时间戳(7 BCD, YYYYMMDDhhmmss)
/// - 0x20 城市(2 BCD)
fn parse_trips(raw: &RawCardData) -> Vec<Trip> {
    let mut trips = Vec::new();
    for n in 1u8..=10 {
        let key = format!("TUnion_trip_{n}");
        let Some(hex) = raw.get(&key) else {
            continue;
        };
        let bytes = hex_to_bytes(hex);
        if bytes.len() < 0x22 {
            continue;
        }
        // 全 0 记录（金额+余额+时间戳均为 0）视为空。
        if be_uint(&bytes, 0x11, 0x19) == 0 && be_uint(&bytes, 0x19, 0x20) == 0 {
            continue;
        }
        let mut trip = Trip::default();
        trip.kind = trip_kind(&hex_slice(&bytes, 0x00, 0x01)).to_string();
        trip.aux = trip_aux(&hex_slice(&bytes, 0x09, 0x0A)).to_string();
        trip.line_station = hex_slice(&bytes, 0x0A, 0x11);
        trip.amount = be_uint(&bytes, 0x11, 0x15) as f64 / 100.0;
        trip.balance = be_uint(&bytes, 0x15, 0x19) as f64 / 100.0;
        // 时间戳 7 字节 BCD：YYYY MM DD hh mm ss。
        let ts = hex_slice(&bytes, 0x19, 0x20);
        trip.timestamp = fmt_timestamp14(&ts);
        trip.city = hex_slice(&bytes, 0x20, 0x22);
        trips.push(trip);
    }
    trips
}

/// 解析卡号与发行/有效期。各卡偏移见文档。
fn parse_card_number(name: &str, raw: &RawCardData, card: &mut ParsedCard) {
    // 北京 BMAC 用 file04，其余用 file15。
    let (file, num, issue, expire) = match name {
        "ShenzhenTong" => (raw.get("ShenzhenTong_file15"), (32, 40), (40, 48), (48, 56)),
        "WuhanTong" => (raw.get("WuhanTong_file15"), (24, 40), (40, 48), (48, 56)),
        "LingnanPass" => (raw.get("LingnanPass_file15"), (22, 32), (0, 0), (0, 0)),
        "CityUnion" => (raw.get("CityUnion_file15"), (24, 40), (40, 48), (48, 56)),
        "TUnion" => (raw.get("TUnion_file15"), (20, 40), (0, 0), (0, 0)),
        "MacauPass" => (raw.get("MacauPass_file15"), (70, 80), (0, 0), (0, 0)),
        "BMAC" => (raw.get("BMAC_file04"), (0, 16), (48, 56), (56, 64)),
        _ => (None, (0, 0), (0, 0), (0, 0)),
    };

    let Some(hex) = file else {
        card.notes.push("未取到基础信息文件".into());
        return;
    };

    if num.1 > 0 {
        let n = slice(hex, num.0, num.1);
        if !n.is_empty() {
            card.number = Some(trim_leading_zeros(n));
        }
    }
    // City Union 城市代码：查城市邮政编码表。
    if name == "CityUnion" {
        let city = slice(hex, 4, 8);
        if !city.is_empty() {
            let v = codes::china_post_code(city)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("未知代码{city}"));
            card.add_field("城市", v);
        }
    }
    if issue.1 > 0 {
        let d = fmt_date8(slice(hex, issue.0, issue.1));
        if !d.is_empty() {
            card.add_field("发行日期", d);
        }
    }
    if expire.1 > 0 {
        let d = fmt_date8(slice(hex, expire.0, expire.1));
        if !d.is_empty() {
            card.add_field("有效期至", d);
        }
    }
}

/// 解析余额。BMAC/深圳等用 `<Name>_balance`；T-Money 用 `TMoney_balance`。
fn parse_balance(name: &str, raw: &RawCardData, card: &mut ParsedCard) {
    let key = format!("{name}_balance");
    let Some(hex) = raw.get(&key) else {
        return;
    };
    let bytes = hex_to_bytes(hex);
    if bytes.len() < 4 {
        card.notes.push("余额字段长度不足".into());
        return;
    }
    if name == "MacauPass" {
        // 澳门通换算：balance*10 - 1000（单位分）。
        let raw_val = be_uint(&bytes, 0, 4) % 0x8000_0000;
        card.balance = Some((raw_val as f64 * 10.0 - 1000.0) / 100.0);
    } else {
        card.balance = Some(pboc_balance_yuan(&bytes));
    }
}

/// 交易类型码 -> 可读名。
fn tti_name(tti: &str) -> &'static str {
    match tti {
        "06" => "消费",
        "02" => "圈存(充值)",
        "09" => "复合消费",
        _ => "其他",
    }
}

/// 岭南通交易记录只提供 MMDD，不可靠包含年份。
fn fmt_month_day4(hex4: &str) -> String {
    if hex4.len() < 4 || !hex4.chars().take(4).all(|c| c.is_ascii_digit()) {
        return String::new();
    }
    let d = &hex4[0..4];
    let month: u32 = d[0..2].parse().unwrap_or(0);
    let day: u32 = d[2..4].parse().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return String::new();
    }
    format!("{}-{}", &d[0..2], &d[2..4])
}

/// 解析交易记录（每条原始 23 字节）。偏移见文档（hex 位）。
fn parse_transactions(name: &str, raw: &RawCardData, card: &mut ParsedCard) {
    for n in 1u8..=10 {
        let key = format!("{name}_trans_{n}");
        let Some(hex) = raw.get(&key) else {
            continue;
        };
        // 全 0 记录视为空。
        if hex.chars().all(|c| c == '0') {
            continue;
        }
        let bytes = hex_to_bytes(hex);
        if bytes.len() < 23 {
            continue;
        }
        let mut t = Transaction::default();
        // 序号 0..4（hex 位）=> 字节 0..2
        t.seq = Some(be_uint(&bytes, 0, 2));
        // 金额 10..18（hex 位）=> 字节 5..9
        let amount_raw = be_uint(&bytes, 5, 9) % 0x8000_0000;
        t.amount = amount_raw as f64 / 100.0;
        if name == "MacauPass" {
            t.amount = amount_raw as f64 * 10.0 / 100.0;
        }
        // 交易类型 18..20（hex 位）=> 字节 9
        let tti = slice(hex, 18, 20);
        t.kind = tti_name(tti).to_string();
        // 消费/复合消费为负、圈存为正。金额为 0 时不加符号，避免 -0。
        if (tti == "06" || tti == "09") && t.amount != 0.0 {
            t.amount = -t.amount;
        }
        // 终端号 20..32（hex 位）=> 字节 10..16
        t.terminal = slice(hex, 20, 32).to_string();
        // 日期 32..40（hex 位）=> 字节 16..20
        t.date = if name == "LingnanPass" {
            fmt_month_day4(slice(hex, 36, 40))
        } else {
            fmt_date8(slice(hex, 32, 40))
        };
        // 时间 40..46（hex 位）=> 字节 20..23
        t.time = fmt_time6(slice(hex, 40, 46));
        card.transactions.push(t);
    }
}

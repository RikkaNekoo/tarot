//! 护照 MRZ 布局（TD3，2 行 × 44 列，ICAO 9303 Part 4）。

use super::super::model::ParsedCard;
use super::mrz;

/// 解析 TD3 护照 MRZ（88 字符）填入卡片字段。
pub fn parse(card: &mut ParsedCard, s: &str) {
    let l1 = &s[0..44];
    let l2 = &s[44..88];

    card.add_field("证件类型", mrz::field(&l1[0..2]));
    card.add_field("签发国", mrz::field(&l1[2..5]));
    let (surname, given) = mrz::split_name(&l1[5..44]);
    card.add_field("姓", surname);
    card.add_field("名", given);

    let number = mrz::field(&l2[0..9]);
    card.number = Some(number.clone());
    card.add_field("证件号", number);
    card.add_field("国籍", mrz::field(&l2[10..13]));
    card.add_field("出生日期", mrz::date(&l2[13..19]));
    card.add_field("性别", mrz::sex(&l2[20..21]));
    card.add_field("有效期至", mrz::date(&l2[21..27]));
}

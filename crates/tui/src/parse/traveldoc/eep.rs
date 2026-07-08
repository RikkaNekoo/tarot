//! 往来港澳通行证 MRZ 布局（TD1 变体，中国特有排布，3 行 × 30 列）。
//!
//! 与 ICAO 标准 TD1 不同，本证把证件号与出生/有效期同置于第 1 行，
//! 顺序为：文档码(2) + 证件号(9) + 校验 + 填充 + 有效期(6) + 校验 + 填充
//! + 出生(6) + 校验 + 填充 + 合成校验。第 2 行前置 12 个附加字符后，
//! 才是姓名的拉丁转写（姓 `<<` 名）。

use super::super::model::ParsedCard;
use super::mrz;

/// 解析 TD1 变体通行证 MRZ（90 字符）填入卡片字段。
pub fn parse(card: &mut ParsedCard, s: &str) {
    let l1 = &s[0..30];
    let l2 = &s[30..60];

    card.add_field("证件类型", mrz::field(&l1[0..2]));

    let number = mrz::field(&l1[2..11]);
    card.number = Some(number.clone());
    card.add_field("证件号", number);

    card.add_field("有效期至", mrz::date(&l1[13..19]));
    card.add_field("出生日期", mrz::date(&l1[21..27]));

    // 第 2 行前 12 位为附加字符，姓名拉丁转写自第 13 位起。
    let (surname, given) = mrz::split_name(&l2[12..]);
    if !surname.is_empty() {
        card.add_field("姓", surname);
    }
    if !given.is_empty() {
        card.add_field("名", given);
    }
}

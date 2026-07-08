//! 电子护照（eMRTD）读取所需的输入参数类型。
//!
//! BAC 认证的密钥由 MRZ（机读区）三要素派生：证件号、出生日期、有效期。
//! 这些无法从芯片读出，必须由用户提供。用强类型承载并封装 MRZ 密钥合成，
//! 供 backend / CLI / TUI 共用。
//!
//! 规范：ICAO Doc 9303 Part 11 §9.7.1.2（MRZ 密钥组成）、Part 3 §4.9（校验位）。

/// 护照 BAC 所需的 MRZ 三要素。
///
/// - `doc_number`：证件号（护照号），不足 9 位时内部以 `<` 右填充。
/// - `date_of_birth`：出生日期，`YYMMDD` 6 位数字。
/// - `date_of_expiry`：有效期至，`YYMMDD` 6 位数字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassportKey {
    pub doc_number: String,
    pub date_of_birth: String,
    pub date_of_expiry: String,
}

impl PassportKey {
    /// 构造。会把证件号转大写并去除内部空格。
    pub fn new(
        doc_number: impl Into<String>,
        date_of_birth: impl Into<String>,
        date_of_expiry: impl Into<String>,
    ) -> Self {
        Self {
            doc_number: doc_number.into().trim().to_uppercase(),
            date_of_birth: date_of_birth.into().trim().to_string(),
            date_of_expiry: date_of_expiry.into().trim().to_string(),
        }
    }

    /// 基本校验：日期须为 6 位数字、证件号非空。
    pub fn validate(&self) -> Result<(), String> {
        if self.doc_number.is_empty() {
            return Err("证件号不能为空".into());
        }
        if self.doc_number.len() > 9 {
            return Err("证件号超过 9 位（如为长号请咨询具体规范）".into());
        }
        if !is_yymmdd(&self.date_of_birth) {
            return Err("出生日期须为 YYMMDD 6 位数字".into());
        }
        if !is_yymmdd(&self.date_of_expiry) {
            return Err("有效期须为 YYMMDD 6 位数字".into());
        }
        Ok(())
    }

    /// 合成 BAC 使用的 MRZ 密钥字符串：
    /// `docNumber(9,补'<') + 校验位 + dob + 校验位 + doe + 校验位`。
    pub fn mrz_key(&self) -> String {
        let padded = pad_doc_number(&self.doc_number);
        let dc = check_digit(&padded);
        let bc = check_digit(&self.date_of_birth);
        let ec = check_digit(&self.date_of_expiry);
        format!(
            "{padded}{dc}{}{bc}{}{ec}",
            self.date_of_birth, self.date_of_expiry
        )
    }
}

/// 是否为 6 位数字。
fn is_yymmdd(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit())
}

/// 证件号右填充 `<` 到 9 位（超长则原样返回）。
fn pad_doc_number(doc: &str) -> String {
    if doc.len() >= 9 {
        doc.to_string()
    } else {
        let mut s = doc.to_string();
        while s.len() < 9 {
            s.push('<');
        }
        s
    }
}

/// ICAO 9303 校验位：权重 7,3,1 循环加权求和 mod 10。
/// 字符取值 0-9→0-9，A-Z→10-35，其余（含 `<`）→0。
pub fn check_digit(input: &str) -> char {
    const W: [u32; 3] = [7, 3, 1];
    let mut sum = 0u32;
    for (i, c) in input.chars().enumerate() {
        let v = if let Some(d) = c.to_digit(10) {
            d
        } else if c.is_ascii_uppercase() {
            (c as u32 - 'A' as u32) + 10
        } else {
            0
        };
        sum += v * W[i % 3];
    }
    char::from(b'0' + (sum % 10) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_digit_icao_example() {
        // ICAO 9303 Part 3 示例：D23145890 -> 校验位 7
        assert_eq!(check_digit("D23145890"), '7');
        // 340727: 3·7+4·3+0·1+7·7+2·3+7·1 = 95 -> 5
        assert_eq!(check_digit("340727"), '5');
        // 有效期 950712 -> 校验位 2
        assert_eq!(check_digit("950712"), '2');
        // 官方 BAC 样例：证件号 L898902C 补位后校验位 3
        assert_eq!(check_digit("L898902C<"), '3');
        // 出生 690806 -> 1，有效期 940623 -> 6
        assert_eq!(check_digit("690806"), '1');
        assert_eq!(check_digit("940623"), '6');
    }

    #[test]
    fn mrz_key_composition() {
        let k = PassportKey::new("L898902C", "690806", "940623");
        // L898902C 补一位 '<' 到 9 位: L898902C<
        let key = k.mrz_key();
        assert!(key.starts_with("L898902C<"));
        // 总长 = 9+1 + 6+1 + 6+1 = 24
        assert_eq!(key.len(), 24);
    }

    #[test]
    fn validate_rejects_bad_date() {
        let k = PassportKey::new("X1234567", "69080", "940623");
        assert!(k.validate().is_err());
    }
}
use crate::error::JSError;
use num_bigint::BigInt;
pub fn parse_bigint_string(raw: &str) -> Result<BigInt, JSError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BigInt::from(0));
    }
    let (sign, after_sign) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i8, rest)
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (1i8, rest)
    } else {
        (1i8, trimmed)
    };
    let (radix, digits) = if after_sign.starts_with("0x") || after_sign.starts_with("0X") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (16, &after_sign[2..])
    } else if after_sign.starts_with("0b") || after_sign.starts_with("0B") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (2, &after_sign[2..])
    } else if after_sign.starts_with("0o") || after_sign.starts_with("0O") {
        if sign != 1 {
            return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
        }
        (8, &after_sign[2..])
    } else {
        (10, after_sign)
    };
    if digits.is_empty() {
        return Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw)));
    }
    match BigInt::parse_bytes(digits.as_bytes(), radix) {
        Some(mut val) => {
            if sign < 0 {
                val = -val;
            }
            Ok(val)
        }
        None => Err(raise_syntax_error!(format!("Cannot convert \"{}\" to a BigInt", raw))),
    }
}

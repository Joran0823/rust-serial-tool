//! HEX 编解码。

/// 解析 HEX 输入为字节序列。
///
/// 规则：
/// - 空白字符与逗号作为分隔符；
/// - 支持 `0x` / `0X` 前缀（如 `0x01 0x0A`）；
/// - 连续无分隔符的偶数个十六进制字符视为字节序列（如 `010AFF`）；
/// - 奇数长度或非法字符返回错误。
pub fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut token = String::new();

    for c in input.chars() {
        if c.is_whitespace() || c == ',' {
            if !token.is_empty() {
                parse_token(&token, &mut out)?;
                token.clear();
            }
            continue;
        }
        token.push(c);
    }
    if !token.is_empty() {
        parse_token(&token, &mut out)?;
    }
    Ok(out)
}

fn parse_token(tok: &str, out: &mut Vec<u8>) -> Result<(), String> {
    let t = tok
        .strip_prefix("0x")
        .or_else(|| tok.strip_prefix("0X"))
        .unwrap_or(tok);
    if t.is_empty() {
        return Err(format!("{tok}: 缺少十六进制数字"));
    }
    if !t.len().is_multiple_of(2) {
        return Err(format!("{tok}: 奇数个十六进制字符"));
    }
    let mut i = 0;
    while i < t.len() {
        let byte = u8::from_str_radix(&t[i..i + 2], 16)
            .map_err(|_| format!("{tok}: 包含非法十六进制字符"))?;
        out.push(byte);
        i += 2;
    }
    Ok(())
}

/// 将字节序列格式化为大写 HEX，字节间以空格分隔。
pub fn format_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        assert_eq!(parse_hex("01 0A FF").unwrap(), vec![0x01, 0x0A, 0xFF]);
    }

    #[test]
    fn parse_contiguous() {
        assert_eq!(parse_hex("010AFF").unwrap(), vec![0x01, 0x0A, 0xFF]);
    }

    #[test]
    fn parse_prefix_and_separators() {
        assert_eq!(parse_hex("0x01, 0x0A").unwrap(), vec![0x01, 0x0A]);
        assert_eq!(parse_hex("0x01\r\n0x0A").unwrap(), vec![0x01, 0x0A]);
    }

    #[test]
    fn parse_lowercase() {
        assert_eq!(parse_hex("ab cd").unwrap(), vec![0xAB, 0xCD]);
    }

    #[test]
    fn parse_odd_length_errors() {
        assert!(parse_hex("0x1").is_err());
        assert!(parse_hex("1").is_err());
        assert!(parse_hex("010").is_err());
    }

    #[test]
    fn parse_invalid_errors() {
        assert!(parse_hex("GG").is_err());
        assert!(parse_hex("0x").is_err());
    }

    #[test]
    fn parse_empty() {
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn format_uppercase_spaced() {
        assert_eq!(format_hex(&[0x01, 0x0a, 0xff]), "01 0A FF");
    }
}

//! 文本编码转换。

use crate::config::TextEncoding;

/// 按指定编码解码字节序列；无法解码的字节以替换符表示。
pub fn decode(bytes: &[u8], enc: TextEncoding) -> String {
    match enc {
        TextEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        TextEncoding::Gbk => {
            let (cow, _, _) = encoding_rs::GBK.decode(bytes);
            cow.into_owned()
        }
        TextEncoding::Ascii => {
            let mut s = String::with_capacity(bytes.len());
            for &b in bytes {
                if b.is_ascii() && !b.is_ascii_control() {
                    s.push(b as char);
                } else {
                    s.push('\u{FFFD}');
                }
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_lossy() {
        assert_eq!(decode(b"hello", TextEncoding::Utf8), "hello");
        assert!(decode(&[0xFF, 0xFE], TextEncoding::Utf8).contains('\u{FFFD}'));
    }

    #[test]
    fn gbk_decode() {
        // "中文" 的 GBK 编码
        let bytes = [0xD6, 0xD0, 0xCE, 0xC4];
        assert_eq!(decode(&bytes, TextEncoding::Gbk), "中文");
    }

    #[test]
    fn ascii_replaces_control() {
        assert_eq!(decode(b"a\x00b\xFF", TextEncoding::Ascii), "a\u{FFFD}b\u{FFFD}");
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

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

/// 按指定编码将文本编码为字节序列；无法表示的字符替换为 `?`。
pub fn encode(text: &str, enc: TextEncoding) -> Vec<u8> {
    match enc {
        TextEncoding::Utf8 => text.as_bytes().to_vec(),
        TextEncoding::Gbk => encoding_rs::GBK.encode(text).0.into_owned(),
        TextEncoding::Ascii => {
            let mut out = Vec::with_capacity(text.len());
            for c in text.chars() {
                if c.is_ascii() {
                    out.push(c as u8);
                } else {
                    out.push(b'?');
                }
            }
            out
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

    #[test]
    fn encode_round_trip_utf8() {
        assert_eq!(encode("中a", TextEncoding::Utf8), "中a".as_bytes());
    }

    #[test]
    fn encode_gbk() {
        assert_eq!(
            encode("中文", TextEncoding::Gbk),
            vec![0xD6, 0xD0, 0xCE, 0xC4]
        );
    }

    #[test]
    fn encode_ascii_replaces_non_ascii() {
        assert_eq!(encode("a中b", TextEncoding::Ascii), b"a?b");
    }
}

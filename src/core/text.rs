use std::borrow::Cow;

use encoding_rs::{EUC_JP, SHIFT_JIS, UTF_16BE, UTF_16LE};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptTextEncoding {
    Utf8,
    Utf8Bom,
    Utf16LittleEndian,
    Utf16LittleEndianBom,
    Utf16BigEndian,
    Utf16BigEndianBom,
    EucJp,
    ShiftJis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedScriptText {
    pub text: String,
    pub encoding: ScriptTextEncoding,
}

#[must_use]
pub fn decode_script_text(bytes: &[u8]) -> String {
    decode_script_text_strict(bytes)
        .map(Cow::into_owned)
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

#[must_use]
pub fn decode_script_text_strict(bytes: &[u8]) -> Option<Cow<'_, str>> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        && let Ok(text) = std::str::from_utf8(&bytes[3..])
    {
        return Some(Cow::Borrowed(text));
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_script_text_with_encoding(bytes).map(|decoded| Cow::Owned(decoded.text));
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Cow::Borrowed(text));
    }

    decode_script_text_with_encoding(bytes).map(|decoded| Cow::Owned(decoded.text))
}

#[must_use]
pub fn decode_script_text_with_encoding(bytes: &[u8]) -> Option<DecodedScriptText> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        && let Ok(text) = std::str::from_utf8(&bytes[3..])
    {
        return Some(DecodedScriptText {
            text: text.to_string(),
            encoding: ScriptTextEncoding::Utf8Bom,
        });
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_with_encoding(
            UTF_16LE,
            &bytes[2..],
            ScriptTextEncoding::Utf16LittleEndianBom,
        );
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_with_encoding(UTF_16BE, &bytes[2..], ScriptTextEncoding::Utf16BigEndianBom);
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(DecodedScriptText {
            text: text.to_string(),
            encoding: ScriptTextEncoding::Utf8,
        });
    }

    decode_with_encoding(EUC_JP, bytes, ScriptTextEncoding::EucJp)
        .or_else(|| decode_with_encoding(SHIFT_JIS, bytes, ScriptTextEncoding::ShiftJis))
        .or_else(|| decode_with_encoding(UTF_16LE, bytes, ScriptTextEncoding::Utf16LittleEndian))
        .or_else(|| decode_with_encoding(UTF_16BE, bytes, ScriptTextEncoding::Utf16BigEndian))
}

#[must_use]
pub fn encode_script_text(text: &str, encoding: ScriptTextEncoding) -> Option<Vec<u8>> {
    match encoding {
        ScriptTextEncoding::Utf8 => Some(text.as_bytes().to_vec()),
        ScriptTextEncoding::Utf8Bom => {
            let mut bytes = Vec::with_capacity(text.len() + 3);
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            bytes.extend_from_slice(text.as_bytes());
            Some(bytes)
        }
        ScriptTextEncoding::Utf16LittleEndian => Some(encode_utf16(text, false, false)),
        ScriptTextEncoding::Utf16LittleEndianBom => Some(encode_utf16(text, false, true)),
        ScriptTextEncoding::Utf16BigEndian => Some(encode_utf16(text, true, false)),
        ScriptTextEncoding::Utf16BigEndianBom => Some(encode_utf16(text, true, true)),
        ScriptTextEncoding::EucJp => encode_with_encoding(EUC_JP, text),
        ScriptTextEncoding::ShiftJis => encode_with_encoding(SHIFT_JIS, text),
    }
}

fn decode_with_encoding(
    encoding: &'static encoding_rs::Encoding,
    bytes: &[u8],
    script_encoding: ScriptTextEncoding,
) -> Option<DecodedScriptText> {
    let (text, _encoding_used, had_errors) = encoding.decode(bytes);
    (!had_errors).then(|| DecodedScriptText {
        text: text.into_owned(),
        encoding: script_encoding,
    })
}

fn encode_with_encoding(encoding: &'static encoding_rs::Encoding, text: &str) -> Option<Vec<u8>> {
    let (bytes, _encoding_used, had_errors) = encoding.encode(text);
    (!had_errors).then(|| bytes.into_owned())
}

fn encode_utf16(text: &str, big_endian: bool, with_bom: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + if with_bom { 2 } else { 0 });
    if with_bom {
        bytes.extend_from_slice(if big_endian {
            &[0xFE, 0xFF]
        } else {
            &[0xFF, 0xFE]
        });
    }
    for code_unit in text.encode_utf16() {
        let unit_bytes = if big_endian {
            code_unit.to_be_bytes()
        } else {
            code_unit.to_le_bytes()
        };
        bytes.extend_from_slice(&unit_bytes);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::{Cow, decode_script_text, decode_script_text_strict};

    #[test]
    fn decodes_shift_jis_script_text() {
        let bytes = [
            0x2f, 0x2f, 0x20, 0x83, 0x58, 0x83, 0x4e, 0x83, 0x8a, 0x83, 0x76, 0x83, 0x67,
        ];

        assert_eq!(decode_script_text(&bytes), "// スクリプト");
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let bytes = [
            0xff, 0xfe, 0x53, 0x00, 0x43, 0x00, 0x52, 0x00, 0x49, 0x00, 0x50, 0x00,
        ];

        assert_eq!(decode_script_text(&bytes), "SCRIP");
    }

    #[test]
    fn borrows_plain_utf8_text() {
        let bytes = b"alert('hello');";

        assert!(matches!(
            decode_script_text_strict(bytes),
            Some(Cow::Borrowed("alert('hello');"))
        ));
    }

    #[test]
    fn borrows_utf8_bom_text_without_bom() {
        let bytes = b"\xef\xbb\xbfalert('hello');";

        assert!(matches!(
            decode_script_text_strict(bytes),
            Some(Cow::Borrowed("alert('hello');"))
        ));
    }
}

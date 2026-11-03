use std::borrow::Cow;

use encoding_rs::{DecoderResult, EUC_JP, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE};

/// Keep the common LF-only source borrowed; normalize CR/CRLF in one pass.
pub(crate) fn normalized_newlines(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            result.push('\n');
        } else {
            result.push(character);
        }
    }
    Cow::Owned(result)
}

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

/// A limited file read must not make an unfinished final character look like
/// an encoding error. Whole-file readers retain their existing strict handling.
pub(crate) fn decode_script_text_prefix(bytes: &[u8], is_complete: bool) -> String {
    if is_complete {
        return decode_script_text(bytes);
    }
    decode_script_text_with_encoding_impl(bytes, false)
        .map(|decoded| decoded.text)
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

    if let Some(decoded) = decode_bomless_utf16_text(bytes, true) {
        return Some(Cow::Owned(decoded.text));
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Cow::Borrowed(text));
    }

    decode_script_text_with_encoding(bytes).map(|decoded| Cow::Owned(decoded.text))
}

#[must_use]
pub fn decode_script_text_with_encoding(bytes: &[u8]) -> Option<DecodedScriptText> {
    decode_script_text_with_encoding_impl(bytes, true)
}

fn decode_script_text_with_encoding_impl(
    bytes: &[u8],
    is_complete: bool,
) -> Option<DecodedScriptText> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        && let Some(decoded) = decode_utf8(&bytes[3..], ScriptTextEncoding::Utf8Bom, is_complete)
    {
        return Some(decoded);
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_with_encoding(
            UTF_16LE,
            &bytes[2..],
            ScriptTextEncoding::Utf16LittleEndianBom,
            is_complete,
        );
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_with_encoding(
            UTF_16BE,
            &bytes[2..],
            ScriptTextEncoding::Utf16BigEndianBom,
            is_complete,
        );
    }

    if let Some(decoded) = decode_bomless_utf16_text(bytes, is_complete) {
        return Some(decoded);
    }

    if let Some(decoded) = decode_utf8(bytes, ScriptTextEncoding::Utf8, is_complete) {
        return Some(decoded);
    }

    decode_legacy_japanese_text(bytes, is_complete)
        .or_else(|| {
            decode_with_encoding(
                UTF_16LE,
                bytes,
                ScriptTextEncoding::Utf16LittleEndian,
                is_complete,
            )
        })
        .or_else(|| {
            decode_with_encoding(
                UTF_16BE,
                bytes,
                ScriptTextEncoding::Utf16BigEndian,
                is_complete,
            )
        })
}

fn decode_utf8(
    bytes: &[u8],
    script_encoding: ScriptTextEncoding,
    is_complete: bool,
) -> Option<DecodedScriptText> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(DecodedScriptText {
            text: text.to_string(),
            encoding: script_encoding,
        });
    }
    if is_complete {
        None
    } else {
        decode_with_encoding(UTF_8, bytes, script_encoding, false)
    }
}

fn decode_bomless_utf16_text(bytes: &[u8], is_complete: bool) -> Option<DecodedScriptText> {
    if bytes.len() < 4 || (is_complete && !bytes.len().is_multiple_of(2)) {
        return None;
    }

    let sampled_pairs = bytes.len().min(1024) / 2;
    let mut even_zeroes = 0usize;
    let mut odd_zeroes = 0usize;
    for pair in bytes[..sampled_pairs * 2].chunks_exact(2) {
        even_zeroes += usize::from(pair[0] == 0);
        odd_zeroes += usize::from(pair[1] == 0);
    }

    let likely_little_endian =
        odd_zeroes * 4 >= sampled_pairs * 3 && even_zeroes * 4 <= sampled_pairs;
    let likely_big_endian = even_zeroes * 4 >= sampled_pairs * 3 && odd_zeroes * 4 <= sampled_pairs;
    if likely_little_endian {
        decode_with_encoding(
            UTF_16LE,
            bytes,
            ScriptTextEncoding::Utf16LittleEndian,
            is_complete,
        )
    } else if likely_big_endian {
        decode_with_encoding(
            UTF_16BE,
            bytes,
            ScriptTextEncoding::Utf16BigEndian,
            is_complete,
        )
    } else {
        None
    }
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
    is_complete: bool,
) -> Option<DecodedScriptText> {
    if is_complete {
        let (text, _encoding_used, had_errors) = encoding.decode(bytes);
        return (!had_errors).then(|| DecodedScriptText {
            text: text.into_owned(),
            encoding: script_encoding,
        });
    }

    let mut decoder = encoding.new_decoder_without_bom_handling();
    let capacity = decoder.max_utf8_buffer_length_without_replacement(bytes.len())?;
    let mut text = String::with_capacity(capacity);
    // last=false buffers an incomplete trailing character, while malformed
    // sequences within the prefix still disqualify this encoding.
    let (result, consumed) = decoder.decode_to_string_without_replacement(bytes, &mut text, false);
    (result == DecoderResult::InputEmpty && consumed == bytes.len()).then_some(DecodedScriptText {
        text,
        encoding: script_encoding,
    })
}

fn decode_legacy_japanese_text(bytes: &[u8], is_complete: bool) -> Option<DecodedScriptText> {
    let euc_jp = decode_with_encoding(EUC_JP, bytes, ScriptTextEncoding::EucJp, is_complete);
    let shift_jis =
        decode_with_encoding(SHIFT_JIS, bytes, ScriptTextEncoding::ShiftJis, is_complete);
    match (euc_jp, shift_jis) {
        (Some(euc_jp), Some(shift_jis)) => {
            let euc_score = decoded_japanese_score(&euc_jp.text);
            let shift_jis_score = decoded_japanese_score(&shift_jis.text);
            if shift_jis_score > euc_score {
                Some(shift_jis)
            } else if euc_score > shift_jis_score {
                Some(euc_jp)
            } else {
                preferred_legacy_japanese_text(euc_jp, shift_jis)
            }
        }
        (Some(euc_jp), None) => Some(euc_jp),
        (None, Some(shift_jis)) => Some(shift_jis),
        (None, None) => None,
    }
}

fn decoded_japanese_score(text: &str) -> i32 {
    text.chars()
        .map(|character| match character {
            '\t' | '\n' | '\r' => 1,
            character if character.is_control() => -20,
            '\u{ff61}'..='\u{ff9f}' => 4,
            '\u{3040}'..='\u{30ff}' => 3,
            '\u{4e00}'..='\u{9fff}' => 1,
            character if character.is_ascii() => 1,
            _ => 0,
        })
        .sum()
}

#[cfg(windows)]
fn preferred_legacy_japanese_text(
    _euc_jp: DecodedScriptText,
    shift_jis: DecodedScriptText,
) -> Option<DecodedScriptText> {
    Some(shift_jis)
}

#[cfg(not(windows))]
fn preferred_legacy_japanese_text(
    euc_jp: DecodedScriptText,
    _shift_jis: DecodedScriptText,
) -> Option<DecodedScriptText> {
    Some(euc_jp)
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
    use super::{
        Cow, ScriptTextEncoding, decode_script_text, decode_script_text_prefix,
        decode_script_text_strict, decode_script_text_with_encoding,
        decode_script_text_with_encoding_impl, decode_with_encoding, encode_script_text,
    };

    #[test]
    fn decodes_shift_jis_script_text() {
        let bytes = [
            0x2f, 0x2f, 0x20, 0x83, 0x58, 0x83, 0x4e, 0x83, 0x8a, 0x83, 0x76, 0x83, 0x67,
        ];

        assert_eq!(decode_script_text(&bytes), "// スクリプト");
    }

    #[test]
    fn prefers_shift_jis_for_half_width_katakana_ambiguity() {
        let bytes = [0xb6, 0xc0, 0xb6, 0xc5];

        let decoded = decode_script_text_with_encoding(&bytes).expect("decode");

        assert_eq!(decoded.encoding, ScriptTextEncoding::ShiftJis);
        assert_eq!(decoded.text, "ｶﾀｶﾅ");
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let bytes = [
            0xff, 0xfe, 0x53, 0x00, 0x43, 0x00, 0x52, 0x00, 0x49, 0x00, 0x50, 0x00,
        ];

        assert_eq!(decode_script_text(&bytes), "SCRIP");
    }

    #[test]
    fn decodes_ascii_heavy_utf16le_without_bom_before_legacy_encodings() {
        let bytes = "SCRIPTMETA-BEGIN\nScript-ID=com.example.utf16\nSCRIPTMETA-END"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        let decoded = decode_script_text_with_encoding(&bytes).expect("decode");

        assert_eq!(decoded.encoding, ScriptTextEncoding::Utf16LittleEndian);
        assert!(decoded.text.contains("Script-ID=com.example.utf16"));
    }

    #[test]
    fn decodes_ascii_heavy_utf16be_without_bom_before_legacy_encodings() {
        let bytes = "SCRIPTMETA-BEGIN\nScript-ID=com.example.utf16be\nSCRIPTMETA-END"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();

        let decoded = decode_script_text_with_encoding(&bytes).expect("decode");

        assert_eq!(decoded.encoding, ScriptTextEncoding::Utf16BigEndian);
        assert!(decoded.text.contains("Script-ID=com.example.utf16be"));
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

    #[test]
    fn partial_utf8_retains_completed_text_at_each_multibyte_boundary() {
        let prefix = "SCRIPTMETA-BEGIN\nName=日本語\nSCRIPTMETA-END\n";
        for encoding in [ScriptTextEncoding::Utf8, ScriptTextEncoding::Utf8Bom] {
            for character in ['¢', '日', '🦀'] {
                let bytes = encode_script_text(&format!("{prefix}{character}"), encoding).unwrap();
                let prefix_len = bytes.len() - character.len_utf8();
                for extra in 1..character.len_utf8() {
                    let decoded =
                        decode_script_text_with_encoding_impl(&bytes[..prefix_len + extra], false)
                            .unwrap();
                    assert_eq!(
                        (decoded.encoding, decoded.text.as_str()),
                        (encoding, prefix)
                    );
                }
            }
        }
    }

    #[test]
    fn partial_utf16_retains_completed_text_at_code_unit_and_surrogate_boundaries() {
        let prefix = "SCRIPTMETA-BEGIN\nName=日本語\nSCRIPTMETA-END\n";
        for encoding in [
            ScriptTextEncoding::Utf16LittleEndian,
            ScriptTextEncoding::Utf16LittleEndianBom,
            ScriptTextEncoding::Utf16BigEndian,
            ScriptTextEncoding::Utf16BigEndianBom,
        ] {
            let bytes = encode_script_text(&format!("{prefix}🦀"), encoding).unwrap();
            for extra in 1..4 {
                let decoded =
                    decode_script_text_with_encoding_impl(&bytes[..bytes.len() - 4 + extra], false)
                        .unwrap();
                assert_eq!(
                    (decoded.encoding, decoded.text.as_str()),
                    (encoding, prefix)
                );
            }
        }
    }

    #[test]
    fn partial_legacy_japanese_retains_completed_text() {
        let prefix = "SCRIPTMETA-BEGIN\nName=日本語のスクリプト\nSCRIPTMETA-END\n";
        for encoding in [ScriptTextEncoding::EucJp, ScriptTextEncoding::ShiftJis] {
            let bytes = encode_script_text(&format!("{prefix}語"), encoding).unwrap();
            let decoded =
                decode_script_text_with_encoding_impl(&bytes[..bytes.len() - 1], false).unwrap();
            assert_eq!(
                (decoded.encoding, decoded.text.as_str()),
                (encoding, prefix)
            );
        }
    }

    #[test]
    fn incomplete_trailing_character_is_only_accepted_for_partial_input() {
        for (encoding, kind, bytes) in [
            (
                encoding_rs::UTF_8,
                ScriptTextEncoding::Utf8,
                b"abc\xe3\x81".as_slice(),
            ),
            (
                encoding_rs::UTF_16LE,
                ScriptTextEncoding::Utf16LittleEndian,
                b"a\0b\0c\0\x3d\xd8".as_slice(),
            ),
            (
                encoding_rs::EUC_JP,
                ScriptTextEncoding::EucJp,
                b"abc\xa4".as_slice(),
            ),
            (
                encoding_rs::SHIFT_JIS,
                ScriptTextEncoding::ShiftJis,
                b"abc\x82".as_slice(),
            ),
        ] {
            assert!(decode_with_encoding(encoding, bytes, kind, true).is_none());
            assert_eq!(
                decode_with_encoding(encoding, bytes, kind, false)
                    .unwrap()
                    .text,
                "abc"
            );
        }
    }

    #[test]
    fn partial_decoder_does_not_ignore_malformed_characters_inside_input() {
        for (encoding, kind, bytes) in [
            (
                encoding_rs::UTF_8,
                ScriptTextEncoding::Utf8,
                b"abc\xffxyz".as_slice(),
            ),
            (
                encoding_rs::UTF_16LE,
                ScriptTextEncoding::Utf16LittleEndian,
                b"a\0\0\xdcx\0".as_slice(),
            ),
            (
                encoding_rs::EUC_JP,
                ScriptTextEncoding::EucJp,
                b"abc\xffxyz".as_slice(),
            ),
            (
                encoding_rs::SHIFT_JIS,
                ScriptTextEncoding::ShiftJis,
                b"abc\x82\x20xyz".as_slice(),
            ),
        ] {
            assert!(decode_with_encoding(encoding, bytes, kind, false).is_none());
        }
    }

    #[test]
    fn complete_prefix_keeps_whole_file_decoding_behavior() {
        for bytes in [
            b"valid UTF-8".as_slice(),
            b"unfinished\xe3\x81",
            b"invalid\xffx",
        ] {
            assert_eq!(
                decode_script_text_prefix(bytes, true),
                decode_script_text(bytes)
            );
        }
    }
}

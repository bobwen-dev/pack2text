use encoding_rs::Encoding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BomKind {
    None,
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl BomKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BomKind::None => "none",
            BomKind::Utf8 => "utf-8",
            BomKind::Utf16Le => "utf-16le",
            BomKind::Utf16Be => "utf-16be",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Some(BomKind::None),
            "utf-8" => Some(BomKind::Utf8),
            "utf-16le" => Some(BomKind::Utf16Le),
            "utf-16be" => Some(BomKind::Utf16Be),
            _ => None,
        }
    }

    pub fn bytes(&self) -> &'static [u8] {
        match self {
            BomKind::None => &[],
            BomKind::Utf8 => &[0xEF, 0xBB, 0xBF],
            BomKind::Utf16Le => &[0xFF, 0xFE],
            BomKind::Utf16Be => &[0xFE, 0xFF],
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectedEncoding {
    pub encoding: &'static Encoding,
    pub bom: BomKind,
}

/// Detect encoding of raw bytes.
/// Returns None if the content is binary or cannot be decoded.
pub fn detect_encoding(bytes: &[u8]) -> Option<DetectedEncoding> {
    // UTF-32 BOMs must be rejected before for_bom maps FF FE 00 00 to UTF-16LE.
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        return None;
    }
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        let bom = bom_from_bytes(&bytes[..bom_len]);
        return Some(DetectedEncoding { encoding, bom });
    }

    if bytes.contains(&0x00) {
        return None;
    }

    let encodings = [
        encoding_rs::UTF_8,
        encoding_rs::GB18030,
        encoding_rs::SHIFT_JIS,
        encoding_rs::EUC_KR,
        encoding_rs::EUC_JP,
        encoding_rs::WINDOWS_1252,
    ];
    for enc in &encodings {
        let (_, _, had_errors) = enc.decode(bytes);
        if !had_errors {
            return Some(DetectedEncoding {
                encoding: enc,
                bom: BomKind::None,
            });
        }
    }

    None
}

/// Detect encoding and return (encoding_name, bom_kind, content_without_bom_as_utf8).
pub fn detect_and_convert(bytes: &[u8]) -> Option<(String, BomKind, String)> {
    let detected = detect_encoding(bytes)?;
    let bom_len = detected.bom.bytes().len();
    let content = &bytes[bom_len..];
    let (cow, _, had_errors) = detected.encoding.decode(content);
    if had_errors {
        return None;
    }
    Some((
        detected.encoding.name().to_ascii_lowercase(),
        detected.bom,
        cow.into_owned(),
    ))
}

/// Re-encode UTF-8 body back to the original charset.
/// Returns None when the charset is unknown or content cannot be represented.
pub fn reencode_to_charset(body: &str, charset: &str) -> Option<Vec<u8>> {
    match charset.to_ascii_lowercase().as_str() {
        "utf-16le" => Some(body.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()),
        "utf-16be" => Some(body.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()),
        _ => {
            let encoding = Encoding::for_label(charset.as_bytes())?;
            let (cow, _, had_errors) = encoding.encode(body);
            if had_errors {
                return None;
            }
            Some(cow.into_owned())
        }
    }
}

fn bom_from_bytes(bytes: &[u8]) -> BomKind {
    match bytes {
        [0xEF, 0xBB, 0xBF, ..] => BomKind::Utf8,
        [0xFF, 0xFE, ..] => BomKind::Utf16Le,
        [0xFE, 0xFF, ..] => BomKind::Utf16Be,
        _ => BomKind::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_utf8_no_bom() {
        let bytes = b"hello world";
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.encoding, encoding_rs::UTF_8);
        assert_eq!(d.bom, BomKind::None);
    }

    #[test]
    fn detect_utf8_bom() {
        let bytes = b"\xEF\xBB\xBFhello";
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.encoding, encoding_rs::UTF_8);
        assert_eq!(d.bom, BomKind::Utf8);
    }

    #[test]
    fn detect_utf16le_bom_with_nul_bytes() {
        let bytes = b"\xFF\xFEh\x00e\x00l\x00l\x00o\x00";
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.bom, BomKind::Utf16Le);
        assert_eq!(d.encoding, encoding_rs::UTF_16LE);
    }

    #[test]
    fn detect_utf16be_bom_with_nul_bytes() {
        let bytes = b"\xFE\xFF\x00h\x00e\x00l\x00l\x00o";
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.bom, BomKind::Utf16Be);
    }

    #[test]
    fn binary_detected() {
        let bytes = &[0x00, 0x01, 0x02];
        assert!(detect_encoding(bytes).is_none());
    }

    #[test]
    fn utf32le_bom_skipped_as_binary() {
        let bytes = &[0xFF, 0xFE, 0x00, 0x00, 0x68, 0x00, 0x00, 0x00];
        assert!(detect_encoding(bytes).is_none());
    }

    #[test]
    fn utf32be_bom_skipped_as_binary() {
        let bytes = &[0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, 0x68];
        assert!(detect_encoding(bytes).is_none());
    }

    #[test]
    fn convert_gbk() {
        let bytes = &[0xC4, 0xE3, 0xBA, 0xC3];
        let (name, bom, text) = detect_and_convert(bytes).unwrap();
        assert!(name.contains("GBK") || name.contains("gb18030"));
        assert_eq!(bom, BomKind::None);
        assert!(text.contains("你好"));
    }

    #[test]
    fn big5_falls_back_to_gb18030_not_skipped() {
        let bytes = &[0xA4, 0xA4, 0xA4, 0xE5]; // 中文 in Big5
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.encoding, encoding_rs::GB18030);
    }

    #[test]
    fn detect_windows1252() {
        let bytes = &[0x63, 0x61, 0x66, 0xE9]; // "café" in cp1252
        let d = detect_encoding(bytes).unwrap();
        assert_eq!(d.encoding, encoding_rs::WINDOWS_1252);
        let (name, bom, text) = detect_and_convert(bytes).unwrap();
        assert_eq!(name, "windows-1252");
        assert_eq!(bom, BomKind::None);
        assert_eq!(text, "café");
    }

    #[test]
    fn convert_utf16le_with_bom() {
        let text = "hello";
        let mut bytes = vec![0xFF, 0xFE];
        for ch in text.encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let (name, bom, converted) = detect_and_convert(&bytes).unwrap();
        assert_eq!(name, "utf-16le");
        assert_eq!(bom, BomKind::Utf16Le);
        assert_eq!(converted, text);
    }

    #[test]
    fn reencode_gbk_roundtrip() {
        let original = &[0xC4, 0xE3, 0xBA, 0xC3];
        let (name, bom, text) = detect_and_convert(original).unwrap();
        assert_eq!(bom, BomKind::None);
        let restored = reencode_to_charset(&text, &name).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn reencode_utf16_roundtrip() {
        let text = "hello";
        let mut original = vec![0xFF, 0xFE];
        for ch in text.encode_utf16() {
            original.extend_from_slice(&ch.to_le_bytes());
        }
        let (name, bom, body) = detect_and_convert(&original).unwrap();
        assert_eq!(bom, BomKind::Utf16Le);
        let mut restored = bom.bytes().to_vec();
        restored.extend_from_slice(&reencode_to_charset(&body, &name).unwrap());
        assert_eq!(restored, original);
    }

    #[test]
    fn reencode_unknown_charset() {
        assert!(reencode_to_charset("x", "klingon-42").is_none());
    }

    #[test]
    fn bom_round_trip() {
        for bom in [
            BomKind::None,
            BomKind::Utf8,
            BomKind::Utf16Le,
            BomKind::Utf16Be,
        ] {
            let s = bom.as_str();
            assert_eq!(BomKind::from_str(s), Some(bom));
        }
    }

    #[test]
    fn bom_from_str_case_insensitive() {
        assert_eq!(BomKind::from_str("UTF-16LE"), Some(BomKind::Utf16Le));
        assert_eq!(BomKind::from_str("None"), Some(BomKind::None));
        assert_eq!(BomKind::from_str("utf-8"), Some(BomKind::Utf8));
        assert_eq!(BomKind::from_str("klingon"), None);
    }
}

use rand::Rng;
use sha2::{Digest, Sha256};
use std::fmt::Write;

use crate::collect::CollectedFile;
use crate::error::{Error, Result};

const BOUNDARY_LEN: usize = 16;

pub fn generate_boundary() -> String {
    let mut rng = rand::thread_rng();
    let hex: String = (0..BOUNDARY_LEN)
        .map(|_| format!("{:02x}", rng.r#gen::<u8>()))
        .collect();
    format!("--=pack2text_{hex}=--")
}

pub fn pack_entry(entry: &CollectedFile, boundary: &str) -> String {
    pack_entry_inner(entry, boundary, false)
}

/// Clipboard-mode entry: only the headers needed to read the text back,
/// since clipboard output is not meant to be unpacked.
pub fn pack_entry_minimal(entry: &CollectedFile, boundary: &str) -> String {
    pack_entry_inner(entry, boundary, true)
}

fn pack_entry_inner(entry: &CollectedFile, boundary: &str, minimal: bool) -> String {
    let mut out = String::with_capacity(512 + entry.utf8_content.len());
    write!(out, "\r\n{boundary}\r\n").unwrap();
    write!(
        out,
        "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
        encode_filename(&entry.rel_path)
    )
    .unwrap();
    if !minimal {
        write!(out, "Content-Type: text/plain; charset=utf-8\r\n").unwrap();
        write!(out, "X-Original-Charset: {}\r\n", entry.original_charset).unwrap();
        write!(out, "X-Original-BOM: {}\r\n", entry.original_bom.as_str()).unwrap();
        write!(out, "X-Original-Size: {}\r\n", entry.original_size).unwrap();
        write!(out, "X-Original-SHA256: {}\r\n", entry.original_sha256).unwrap();
    }
    write!(out, "X-Content-Length: {}\r\n", entry.utf8_content.len()).unwrap();
    write!(out, "\r\n").unwrap();
    out.push_str(&entry.utf8_content);
    out
}

pub fn pack_header(boundary: &str) -> String {
    format!("{}\r\n", boundary)
}

pub fn pack_footer(boundary: &str) -> String {
    format!("\r\n{boundary}--\r\n")
}

pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    result.iter().fold(String::with_capacity(64), |mut acc, b| {
        write!(acc, "{b:02x}").unwrap();
        acc
    })
}

pub struct ParsedEntry {
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub fn parse_entries(container: &str) -> Result<Vec<ParsedEntry>> {
    let boundary = find_boundary(container)?;
    let part_start = format!("\r\n{boundary}\r\n");
    let end_marker = format!("\r\n{boundary}--\r\n");

    let mut entries = Vec::new();
    let mut pos = boundary.len() + 2;
    if pos > container.len() {
        return Err(Error::ContainerParse {
            message: "truncated container".to_string(),
        });
    }

    loop {
        let rest = &container[pos..];
        if rest.starts_with(&end_marker) {
            let after = pos + end_marker.len();
            if after != container.len() {
                return Err(Error::ContainerParse {
                    message: "trailing content after footer".to_string(),
                });
            }
            break;
        }
        if !rest.starts_with(&part_start) {
            return Err(Error::ContainerParse {
                message: "expected part boundary".to_string(),
            });
        }

        pos += part_start.len();
        let rest = &container[pos..];
        let header_end = rest.find("\r\n\r\n").ok_or_else(|| Error::ContainerParse {
            message: "missing header terminator".to_string(),
        })?;
        let header_section = &rest[..header_end];

        let mut headers = Vec::new();
        for line in header_section.lines() {
            if let Some((key, value)) = line.split_once(':') {
                headers.push((key.trim().to_string(), value.trim().to_string()));
            }
        }

        let content_length: usize = {
            let raw = get_header(&headers, "X-Content-Length")?;
            raw.parse().map_err(|_| Error::InvalidHeader {
                header: "X-Content-Length".to_string(),
                value: raw,
            })?
        };

        let body_start = match pos.checked_add(header_end).and_then(|x| x.checked_add(4)) {
            Some(v) => v,
            None => {
                return Err(Error::ContainerParse {
                    message: "body start overflow".to_string(),
                });
            }
        };
        let Some(body_end) = body_start.checked_add(content_length) else {
            return Err(Error::ContainerParse {
                message: "X-Content-Length overflow".to_string(),
            });
        };
        if body_end > container.len() {
            return Err(Error::ContainerParse {
                message: "body exceeds container length".to_string(),
            });
        }
        let body = &container[body_start..body_end];
        entries.push(ParsedEntry {
            headers,
            body: body.to_string(),
        });
        pos = body_end;
    }

    Ok(entries)
}

fn find_boundary(container: &str) -> Result<String> {
    let first_line_end = container.find("\r\n").or_else(|| container.find('\n'));
    let first_line = match first_line_end {
        Some(idx) => &container[..idx],
        None => container,
    };
    if first_line.starts_with("--=pack2text_") && first_line.ends_with("=--") {
        Ok(first_line.to_string())
    } else {
        Err(Error::InvalidBoundary)
    }
}

fn encode_filename(path: &str) -> String {
    let mut out = String::with_capacity(path.len() * 3);
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => {
                write!(out, "%{byte:02X}").unwrap();
            }
        }
    }
    out
}

pub fn decode_filename(encoded: &str) -> Result<String> {
    let mut out = Vec::new();
    let bytes = encoded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &encoded[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).map_err(|_| Error::ContainerParse {
        message: format!("invalid UTF-8 in path: {encoded}"),
    })
}

pub fn get_header(headers: &[(String, String)], key: &str) -> Result<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .ok_or_else(|| Error::MissingHeader {
            header: key.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::CollectedFile;
    use crate::encoding::BomKind;

    fn utf8_entry(rel_path: &str, content: &str) -> CollectedFile {
        CollectedFile {
            rel_path: rel_path.to_string(),
            original_charset: "utf-8".to_string(),
            original_bom: BomKind::None,
            original_size: content.len() as u64,
            original_sha256: compute_sha256_hex(content.as_bytes()),
            utf8_content: content.to_string(),
        }
    }

    #[test]
    fn boundary_format() {
        let b = generate_boundary();
        assert!(b.starts_with("--=pack2text_"));
        assert!(b.ends_with("=--"));
        assert_eq!(b.len(), 48);
    }

    #[test]
    fn filename_encode_decode_roundtrip() {
        let paths = vec![
            "hello.txt",
            "a b.txt",
            "中文.txt",
            "path/to/file.txt",
            "file'quote.txt",
        ];
        for p in paths {
            let encoded = encode_filename(p);
            let decoded = decode_filename(&encoded).unwrap();
            assert_eq!(decoded, p);
        }
    }

    #[test]
    fn pack_parse_roundtrip() {
        let boundary = generate_boundary();
        let entry = utf8_entry("src/main.rs", "hello world\n");

        let mut container = String::new();
        container.push_str(&pack_header(&boundary));
        container.push_str(&pack_entry(&entry, &boundary));
        container.push_str(&pack_footer(&boundary));

        let entries = parse_entries(&container).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body, "hello world\n");

        let charset = get_header(&entries[0].headers, "X-Original-Charset").unwrap();
        assert_eq!(charset, "utf-8");

        let bom = get_header(&entries[0].headers, "X-Original-BOM").unwrap();
        assert_eq!(bom, "none");
    }

    #[test]
    fn pack_entry_minimal_keeps_only_basic_headers() {
        let boundary = generate_boundary();
        let entry = utf8_entry("src/main.rs", "hello world\n");
        let part = pack_entry_minimal(&entry, &boundary);
        assert!(part.contains("Content-Disposition: form-data"));
        assert!(part.contains("X-Content-Length: 12"));
        assert!(!part.contains("X-Original-Charset"));
        assert!(!part.contains("X-Original-BOM"));
        assert!(!part.contains("X-Original-Size"));
        assert!(!part.contains("X-Original-SHA256"));
        assert!(part.ends_with("hello world\n"));
    }

    #[test]
    fn unpack_rejects_minimal_clipboard_container() {
        let boundary = generate_boundary();
        let entry = utf8_entry("src/main.rs", "hello");
        let mut container = String::new();
        container.push_str(&pack_header(&boundary));
        container.push_str(&pack_entry_minimal(&entry, &boundary));
        container.push_str(&pack_footer(&boundary));
        let result = crate::unpack::unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::MissingHeader { .. })));
    }

    #[test]
    fn multiple_entries() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&pack_header(&boundary));

        for i in 0..5 {
            let entry = utf8_entry(&format!("file{i}.txt"), "test");
            container.push_str(&pack_entry(&entry, &boundary));
        }
        container.push_str(&pack_footer(&boundary));

        let entries = parse_entries(&container).unwrap();
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn parse_rejects_no_boundary() {
        let result = parse_entries("no boundary here");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_bare_boundary_without_newline() {
        let bare = "--=pack2text_0123456789abcdef0123456789abcdef=--";
        let result = parse_entries(bare);
        assert!(result.is_err());
    }

    #[test]
    fn boundary_string_inside_body_is_safe() {
        let boundary = generate_boundary();
        let body = format!("line1\r\n{boundary}\r\nline2\r\n{boundary}--\r\ntail");
        let entry = utf8_entry("tricky.txt", &body);

        let mut container = String::new();
        container.push_str(&pack_header(&boundary));
        container.push_str(&pack_entry(&entry, &boundary));
        container.push_str(&pack_footer(&boundary));

        let entries = parse_entries(&container).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body, body);
    }

    #[test]
    fn huge_content_length_returns_error_not_panic() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-Original-Charset: utf-8\r\n\
             X-Original-BOM: none\r\n\
             X-Original-Size: 5\r\n\
             X-Original-SHA256: abc\r\n\
             X-Content-Length: {}\r\n\
             \r\nhello",
            usize::MAX
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let result = parse_entries(&container);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_truncated_body() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-Original-Charset: utf-8\r\n\
             X-Original-BOM: none\r\n\
             X-Original-Size: 100\r\n\
             X-Original-SHA256: abc\r\n\
             X-Content-Length: 100\r\n\
             \r\nshort"
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let result = parse_entries(&container);
        assert!(result.is_err());
    }

    proptest::proptest! {
        #[test]
        fn pack_parse_roundtrip_arbitrary_bodies(
            body in "\\PC*",
            rel_path in "[a-z0-9_/.]{1,80}",
        ) {
            let boundary = generate_boundary();
            let entry = utf8_entry(&rel_path, &body);
            let mut container = String::new();
            container.push_str(&pack_header(&boundary));
            container.push_str(&pack_entry(&entry, &boundary));
            container.push_str(&pack_footer(&boundary));

            let entries = parse_entries(&container).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].body, body);
        }

        #[test]
        fn parse_never_panics_on_arbitrary_input(
            s in "\\PC*",
        ) {
            let _ = parse_entries(&s);
        }

        #[test]
        fn parse_never_panics_with_arbitrary_content_length(
            content_length in 0usize..=usize::MAX,
        ) {
            let boundary = generate_boundary();
            let mut container = String::new();
            container.push_str(&pack_header(&boundary));
            container.push_str(&format!(
                "\r\n{boundary}\r\n\
                 Content-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\
                 X-Original-Charset: utf-8\r\n\
                 X-Original-BOM: none\r\n\
                 X-Original-Size: 5\r\n\
                 X-Original-SHA256: abc\r\n\
                 X-Content-Length: {content_length}\r\n\
                 \r\nhello"
            ));
            container.push_str(&format!("\r\n{boundary}--\r\n"));
            let _ = parse_entries(&container);
        }
    }
}

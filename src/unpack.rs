use std::path::Path;

use crate::encoding::{self, BomKind};
use crate::error::{Error, Result};
use crate::format;

pub struct UnpackedFile {
    pub rel_path: String,
    pub original_charset: String,
    pub original_bom: BomKind,
    pub original_size: u64,
    pub original_sha256: String,
    pub body: String,
}

pub fn unpack_to_memory(container: &str) -> Result<Vec<UnpackedFile>> {
    let entries = format::parse_entries(container)?;
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for entry in entries {
        let disposition = format::get_header(&entry.headers, "Content-Disposition")?;
        let rel_path = extract_filename(&disposition)?;
        validate_path(&rel_path)?;
        if !seen.insert(rel_path.clone()) {
            return Err(Error::DuplicatePath { path: rel_path });
        }

        let original_charset = format::get_header(&entry.headers, "X-Original-Charset")?;
        let bom_str = format::get_header(&entry.headers, "X-Original-BOM")?;
        let original_bom = BomKind::from_str(&bom_str).ok_or_else(|| Error::InvalidHeader {
            header: "X-Original-BOM".to_string(),
            value: bom_str.clone(),
        })?;
        let original_size = parse_u64_header(&entry.headers, "X-Original-Size")?;
        let original_sha256 = format::get_header(&entry.headers, "X-Original-SHA256")?;

        files.push(UnpackedFile {
            rel_path,
            original_charset,
            original_bom,
            original_size,
            original_sha256,
            body: entry.body,
        });
    }

    Ok(files)
}

pub fn unpack_to_dir(container: &str, output_dir: &Path) -> Result<usize> {
    let files = unpack_to_memory(container)?;
    let count = files.len();

    let mut staged: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::with_capacity(count);
    for file in &files {
        let target = output_dir.join(&file.rel_path);
        if !target.starts_with(output_dir) {
            return Err(Error::PathTraversal {
                path: file.rel_path.clone(),
            });
        }
        if target.exists() {
            return Err(Error::FileExists {
                path: file.rel_path.clone(),
            });
        }
        let bytes = restore_original_bytes(file)?;
        verify_integrity(file, &bytes)?;
        staged.push((target, bytes));
    }

    std::fs::create_dir_all(output_dir)?;
    let out_canonical = output_dir.canonicalize()?;

    let mut written: Vec<std::path::PathBuf> = Vec::new();
    let mut canonical_cache: std::collections::HashMap<std::path::PathBuf, std::path::PathBuf> =
        std::collections::HashMap::new();
    let write_result = (|| -> Result<()> {
        for (target, bytes) in &staged {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
                let parent_canonical = match canonical_cache.get(parent) {
                    Some(c) => c.clone(),
                    None => {
                        let c = parent.canonicalize()?;
                        canonical_cache.insert(parent.to_path_buf(), c.clone());
                        c
                    }
                };
                if !parent_canonical.starts_with(&out_canonical) {
                    return Err(Error::PathTraversal {
                        path: target.display().to_string(),
                    });
                }
            }
            written.push(target.clone());
            std::fs::write(target, bytes)?;
        }
        Ok(())
    })();
    if let Err(e) = write_result {
        for w in written.iter().rev() {
            if let Err(rm_err) = std::fs::remove_file(w) {
                eprintln!("warning: failed to roll back {}: {rm_err}", w.display());
            }
        }
        return Err(e);
    }

    Ok(count)
}

/// Rebuild original bytes from container body: UTF-8 stays as-is, other
/// charsets are re-encoded back. Unknown or unrepresentable charsets are a
/// hard error — silently degrading to UTF-8 would corrupt data undetected.
fn restore_original_bytes(file: &UnpackedFile) -> Result<Vec<u8>> {
    let mut bytes = file.original_bom.bytes().to_vec();
    if file.original_charset.eq_ignore_ascii_case("utf-8") {
        bytes.extend_from_slice(file.body.as_bytes());
    } else {
        let reencoded = encoding::reencode_to_charset(&file.body, &file.original_charset)
            .ok_or_else(|| Error::InvalidHeader {
                header: "X-Original-Charset".to_string(),
                value: file.original_charset.clone(),
            })?;
        bytes.extend_from_slice(&reencoded);
    }
    Ok(bytes)
}

fn verify_integrity(file: &UnpackedFile, restored: &[u8]) -> Result<()> {
    if restored.len() as u64 != file.original_size {
        return Err(Error::SizeMismatch {
            path: file.rel_path.clone(),
            expected: file.original_size,
            actual: restored.len() as u64,
        });
    }
    let actual_sha = format::compute_sha256_hex(restored);
    if actual_sha != file.original_sha256 {
        return Err(Error::IntegrityMismatch {
            path: file.rel_path.clone(),
            expected: file.original_sha256.clone(),
            actual: actual_sha,
        });
    }
    Ok(())
}

fn parse_u64_header(headers: &[(String, String)], key: &str) -> Result<u64> {
    let value = format::get_header(headers, key)?;
    value.parse::<u64>().map_err(|_| Error::InvalidHeader {
        header: key.to_string(),
        value: value.clone(),
    })
}

fn extract_filename(header_value: &str) -> Result<String> {
    let start = header_value
        .to_ascii_lowercase()
        .find("filename=")
        .map(|p| p + 9)
        .ok_or_else(|| Error::InvalidHeader {
            header: "Content-Disposition".to_string(),
            value: header_value.to_string(),
        })?;
    let name = header_value[start..].trim_start();
    let end = if let Some(inner) = name.strip_prefix('"') {
        inner
            .find('"')
            .map(|p| p + 1)
            .ok_or_else(|| Error::InvalidHeader {
                header: "Content-Disposition".to_string(),
                value: header_value.to_string(),
            })?
    } else {
        name.find(';').unwrap_or(name.len())
    };
    let raw = if name.starts_with('"') {
        &name[1..end]
    } else {
        name[..end].trim_end()
    };
    format::decode_filename(raw)
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(Error::ContainerParse {
            message: "empty file path".to_string(),
        });
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(Error::PathTraversal {
            path: path.to_string(),
        });
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(Error::PathTraversal {
            path: path.to_string(),
        });
    }
    if path.split(['/', '\\']).any(|seg| {
        seg == ".."
            || (seg.len() == 2
                && seg.as_bytes()[0].is_ascii_alphabetic()
                && seg.as_bytes()[1] == b':')
            || seg.contains(':')
    }) {
        return Err(Error::PathTraversal {
            path: path.to_string(),
        });
    }
    if path.split(['/', '\\']).any(|seg| {
        seg.is_empty()
            || seg
                .bytes()
                .any(|b| b < 0x20 || matches!(b, b'"' | b'<' | b'>' | b'|' | b'?' | b'*'))
            || seg.ends_with('.')
            || seg.ends_with(' ')
            || seg.chars().count() > 255
            || crate::collect::is_windows_reserved_name(seg)
    }) {
        return Err(Error::ContainerParse {
            message: format!("invalid characters in path: {path}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::CollectedFile;
    use crate::format::generate_boundary;
    use std::fs;
    use tempfile::TempDir;

    fn build_container(entries: &[CollectedFile]) -> String {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&format::pack_header(&boundary));
        for entry in entries {
            container.push_str(&format::pack_entry(entry, &boundary));
        }
        container.push_str(&format::pack_footer(&boundary));
        container
    }

    fn utf8_entry(rel_path: &str, content: &str) -> CollectedFile {
        CollectedFile {
            rel_path: rel_path.to_string(),
            original_charset: "utf-8".to_string(),
            original_bom: BomKind::None,
            original_size: content.len() as u64,
            original_sha256: format::compute_sha256_hex(content.as_bytes()),
            utf8_content: content.to_string(),
        }
    }

    #[test]
    fn unpack_single_file() {
        let container = build_container(&[utf8_entry("hello.txt", "hello")]);
        let files = unpack_to_memory(&container).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].body, "hello");
        assert_eq!(files[0].rel_path, "hello.txt");
    }

    #[test]
    fn unpack_to_dir_creates_files() {
        let container = build_container(&[utf8_entry("src/main.rs", "fn main() {}")]);
        let out = TempDir::new().unwrap();
        let count = unpack_to_dir(&container, out.path()).unwrap();
        assert_eq!(count, 1);
        let content = fs::read_to_string(out.path().join("src/main.rs")).unwrap();
        assert_eq!(content, "fn main() {}");
    }

    #[test]
    fn roundtrip_utf8_pack_unpack() {
        let content = "fn main() {\n    println!(\"hi\");\n}\n";
        let container = build_container(&[utf8_entry("proj/main.rs", content)]);
        let out = TempDir::new().unwrap();
        unpack_to_dir(&container, out.path()).unwrap();
        let restored = fs::read_to_string(out.path().join("proj/main.rs")).unwrap();
        assert_eq!(restored, content);
    }

    #[test]
    fn roundtrip_gbk_reencodes_exactly() {
        let original = &[0xC4, 0xE3, 0xBA, 0xC3];
        let (name, bom, body) = encoding::detect_and_convert(original).unwrap();
        assert_eq!(bom, BomKind::None);
        let entry = CollectedFile {
            rel_path: "中文.txt".to_string(),
            original_charset: name,
            original_bom: bom,
            original_size: original.len() as u64,
            original_sha256: format::compute_sha256_hex(original),
            utf8_content: body,
        };
        let container = build_container(&[entry]);
        let out = TempDir::new().unwrap();
        unpack_to_dir(&container, out.path()).unwrap();
        let restored = fs::read(out.path().join("中文.txt")).unwrap();
        assert_eq!(restored, original);
    }

    #[test]
    fn tampered_body_fails_integrity() {
        let mut entry = utf8_entry("hello.txt", "hello");
        entry.original_sha256 = format::compute_sha256_hex(b"tampered");
        let container = build_container(&[entry]);
        let out = TempDir::new().unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(matches!(result, Err(Error::IntegrityMismatch { .. })));
    }

    #[test]
    fn tampered_size_fails_verification() {
        let mut entry = utf8_entry("hello.txt", "hello");
        entry.original_size = 999;
        let container = build_container(&[entry]);
        let out = TempDir::new().unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(matches!(result, Err(Error::SizeMismatch { .. })));
    }

    #[test]
    fn rejects_path_traversal() {
        let mut entry = utf8_entry("../../etc/passwd", "hello");
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_absolute_path() {
        let mut entry = utf8_entry("/etc/passwd", "hello");
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_path() {
        let mut entry = utf8_entry("", "hello");
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(result.is_err());
    }

    #[test]
    fn missing_charset_header_fails() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&format::pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-Original-BOM: none\r\n\
             X-Original-Size: 5\r\n\
             X-Original-SHA256: abc\r\n\
             X-Content-Length: 5\r\n\
             \r\nhello"
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::MissingHeader { .. })));
    }

    #[test]
    fn content_length_mismatch_detected() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&format::pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"x.txt\"\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-Original-Charset: utf-8\r\n\
             X-Original-BOM: none\r\n\
             X-Original-Size: 5\r\n\
             X-Original-SHA256: abc\r\n\
             X-Content-Length: 100\r\n\
             \r\nhello"
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::ContainerParse { .. })));
    }

    #[test]
    fn rejects_drive_letter_path() {
        let mut entry = utf8_entry("C:/evil.txt", "hello");
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::PathTraversal { .. })));
    }

    #[test]
    fn rejects_drive_letter_backslash_path() {
        let mut entry = utf8_entry("C:%5Cevil.txt", "hello"); // decodes to "C:\evil.txt"
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::PathTraversal { .. })));
    }

    #[test]
    fn rejects_colon_in_component() {
        for path in ["a/C:evil.txt", "C:foo.txt", "x:y.txt"] {
            let mut entry = utf8_entry(path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path {path:?} not rejected"
            );
        }
    }

    #[test]
    fn rejects_windows_reserved_names() {
        for path in ["con.txt", "CON", "aux.log", "com1", "lpt9.bin", "dir/nul"] {
            let mut entry = utf8_entry(path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path {path:?} not rejected"
            );
        }
    }

    #[test]
    fn accepts_normal_names_even_with_con_prefix() {
        for path in ["console.txt", "count.bin", "com10.txt", "lpt99.txt"] {
            let entry = utf8_entry(path, "x");
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_ok(),
                "path {path:?} wrongly rejected"
            );
        }
    }

    #[test]
    fn rejects_component_longer_than_windows_limit() {
        let long = "a".repeat(256);
        for path in [format!("{long}.txt"), format!("dir/{long}")] {
            let mut entry = utf8_entry(&path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path with 256-char component not rejected"
            );
        }
    }

    #[test]
    fn accepts_component_at_windows_limit() {
        let ok = format!("{}.txt", "a".repeat(251));
        assert_eq!(ok.chars().count(), 255);
        let entry = utf8_entry(&ok, "x");
        let container = build_container(&[entry]);
        assert!(unpack_to_memory(&container).is_ok());
    }

    #[test]
    fn rejects_duplicate_paths() {
        let container = build_container(&[utf8_entry("a.txt", "x"), utf8_entry("a.txt", "y")]);
        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::DuplicatePath { .. })));
    }

    #[test]
    fn unknown_charset_fails_hard() {
        let mut entry = utf8_entry("a.txt", "hello");
        entry.original_charset = "klingon-42".to_string();
        let container = build_container(&[entry]);
        let out = TempDir::new().unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(result.is_err());
        assert!(!out.path().join("a.txt").exists());
    }

    #[test]
    fn verify_failure_writes_nothing() {
        let good1 = utf8_entry("a.txt", "good");
        let good2 = utf8_entry("b.txt", "nice");
        let mut bad = utf8_entry("c.txt", "tampered");
        bad.original_sha256 = format::compute_sha256_hex(b"XXXX");
        let container = build_container(&[good1, good2, bad]);
        let out = TempDir::new().unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(result.is_err());
        assert!(!out.path().join("a.txt").exists());
        assert!(!out.path().join("b.txt").exists());
        assert!(!out.path().join("c.txt").exists());
    }

    #[test]
    fn roundtrip_dotdot_in_filename() {
        let container = build_container(&[utf8_entry("proj/a..b.txt", "hello")]);
        let out = TempDir::new().unwrap();
        unpack_to_dir(&container, out.path()).unwrap();
        let restored = fs::read(out.path().join("proj/a..b.txt")).unwrap();
        assert_eq!(restored, b"hello");
    }

    #[test]
    fn rejects_dotdot_component() {
        let mut entry = utf8_entry("a/../b.txt", "hello");
        entry.original_sha256 = String::new();
        let container = build_container(&[entry]);
        let result = unpack_to_memory(&container);
        assert!(matches!(result, Err(Error::PathTraversal { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_dir_escape() {
        use std::os::unix::fs::symlink;
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let out = base.path().join("out");
        fs::create_dir_all(&out).unwrap();
        symlink(outside.path(), out.join("link")).unwrap();
        let container = build_container(&[utf8_entry("link/victim.txt", "x")]);
        let result = unpack_to_dir(&container, &out);
        assert!(result.is_err());
        assert!(!outside.path().join("victim.txt").exists());
    }

    #[test]
    fn refuses_to_overwrite_existing_file() {
        let container = build_container(&[utf8_entry("v.txt", "NEWDATA")]);
        let out = TempDir::new().unwrap();
        fs::write(out.path().join("v.txt"), b"OLD").unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(matches!(result, Err(Error::FileExists { .. })));
        let content = fs::read_to_string(out.path().join("v.txt")).unwrap();
        assert_eq!(content, "OLD");
    }

    #[test]
    fn write_failure_rolls_back_written_files() {
        let container = build_container(&[utf8_entry("a.txt", "x"), utf8_entry("b/x.txt", "y")]);
        let out = TempDir::new().unwrap();
        fs::write(out.path().join("b"), b"file-not-dir").unwrap();
        let result = unpack_to_dir(&container, out.path());
        assert!(result.is_err());
        assert!(!out.path().join("a.txt").exists());
    }

    #[test]
    fn rejects_control_chars_in_path() {
        for ch in ['\0', '\n', '\r', '\u{1F}'] {
            let path = format!("a{ch}b.txt");
            let mut entry = utf8_entry(&path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path {path:?} not rejected"
            );
        }
    }

    #[test]
    fn rejects_windows_reserved_chars_in_path() {
        for ch in ['"', '<', '>', '|', '?', '*'] {
            let path = format!("a{ch}b.txt");
            let mut entry = utf8_entry(&path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path {path:?} not rejected"
            );
        }
    }

    #[test]
    fn rejects_trailing_dot_or_space_in_component() {
        for path in ["file.", "dir/file.txt.", "dir ./x.txt", "file "] {
            let mut entry = utf8_entry(path, "x");
            entry.original_sha256 = String::new();
            let container = build_container(&[entry]);
            assert!(
                unpack_to_memory(&container).is_err(),
                "path {path:?} not rejected"
            );
        }
    }

    #[test]
    fn accepts_filename_without_quotes() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&format::pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; FILENAME=plain.txt\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-Original-Charset: utf-8\r\n\
             X-Original-BOM: none\r\n\
             X-Original-Size: 5\r\n\
             X-Original-SHA256: {}\r\n\
             X-Content-Length: 5\r\n\
             \r\nhello",
            format::compute_sha256_hex(b"hello")
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let files = unpack_to_memory(&container).unwrap();
        assert_eq!(files[0].rel_path, "plain.txt");
    }

    #[test]
    fn accepts_uppercase_header_names() {
        let boundary = generate_boundary();
        let mut container = String::new();
        container.push_str(&format::pack_header(&boundary));
        container.push_str(&format!(
            "\r\n{boundary}\r\n\
             CONTENT-DISPOSITION: form-data; name=\"file\"; filename=\"u.txt\"\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\
             X-ORIGINAL-CHARSET: utf-8\r\n\
             X-ORIGINAL-BOM: none\r\n\
             X-ORIGINAL-SIZE: 5\r\n\
             X-ORIGINAL-SHA256: {}\r\n\
             X-CONTENT-LENGTH: 5\r\n\
             \r\nhello",
            format::compute_sha256_hex(b"hello")
        ));
        container.push_str(&format!("\r\n{boundary}--\r\n"));

        let files = unpack_to_memory(&container).unwrap();
        assert_eq!(files[0].rel_path, "u.txt");
        assert_eq!(files[0].body, "hello");
    }
}

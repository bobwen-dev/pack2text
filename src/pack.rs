use std::path::{Path, PathBuf};

use crate::collect::{self, CollectedFile};
use crate::error::Result;
use crate::format;

pub struct PackResult {
    pub container: String,
    pub file_count: usize,
}

pub fn default_output_name(directories: &[PathBuf]) -> String {
    let first = directories
        .first()
        .map(|p| p.as_path())
        .unwrap_or(Path::new("output"));
    let dir_name = first
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    format!("{dir_name}.txt")
}

pub fn pack_selected(
    selected: &[&Path],
    include: Option<&[String]>,
    exclude: Option<&[String]>,
    output: Option<&Path>,
    clipboard: bool,
) -> Result<PackResult> {
    let files = collect::collect_selected(selected, include, exclude, output)?;
    if files.is_empty() {
        return Err(crate::error::Error::NoTextFiles);
    }
    pack_files(&files, clipboard)
}

pub fn menu_output_location(selected: &[&Path], menu_dir: Option<&Path>) -> (PathBuf, String) {
    let ancestor = collect::common_ancestor(selected);
    let name = ancestor
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .unwrap_or_else(|| "output".to_string());
    let dir = menu_dir
        .filter(|d| d.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| {
            if selected.len() == 1 && selected[0].is_dir() {
                selected[0]
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."))
            } else {
                ancestor
            }
        });
    (dir, format!("{name}.txt"))
}

pub fn resolve_auto_rename(target: &Path) -> PathBuf {
    if !target.exists() {
        return target.to_path_buf();
    }
    let dir = target.parent().unwrap_or(Path::new("."));
    let file = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (file, String::new()),
    };
    let mut n = 1;
    loop {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

pub fn pack_directories(
    roots: &[&Path],
    include: Option<&[String]>,
    exclude: Option<&[String]>,
    output: Option<&Path>,
    clipboard: bool,
) -> Result<PackResult> {
    let mut all_files = Vec::new();
    for root in roots {
        let mut files = collect::collect_text_files(root, include, exclude, output)?;
        all_files.append(&mut files);
    }
    if all_files.is_empty() {
        return Err(crate::error::Error::NoTextFiles);
    }
    pack_files(&all_files, clipboard)
}

fn pack_files(files: &[CollectedFile], clipboard: bool) -> Result<PackResult> {
    let boundary = format::generate_boundary();
    let file_count = files.len();

    let mut seen = std::collections::HashSet::new();
    for file in files {
        if !seen.insert(file.rel_path.as_str()) {
            return Err(crate::error::Error::DuplicatePath {
                path: file.rel_path.clone(),
            });
        }
    }

    let mut container = String::with_capacity(
        files
            .iter()
            .map(|f| 512 + f.utf8_content.len())
            .sum::<usize>()
            + 256,
    );

    container.push_str(&format::pack_header(&boundary));

    for file in files {
        if clipboard {
            container.push_str(&format::pack_entry_minimal(file, &boundary));
        } else {
            container.push_str(&format::pack_entry(file, &boundary));
        }
    }

    container.push_str(&format::pack_footer(&boundary));

    Ok(PackResult {
        container,
        file_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_dir(files: &[(&str, &[u8])]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }
        dir
    }

    #[test]
    fn pack_single_file() {
        let d = setup_dir(&[("hello.txt", b"hello world")]);
        let root = d.path();
        let result = pack_directories(&[root], None, None, None, false).unwrap();
        assert_eq!(result.file_count, 1);
        assert!(result.container.contains("hello world"));
        assert!(result.container.starts_with("--=pack2text_"));
    }

    #[test]
    fn pack_multiple_files() {
        let d = setup_dir(&[("a.txt", b"aaa"), ("b.txt", b"bbb"), ("c.txt", b"ccc")]);
        let root = d.path();
        let result = pack_directories(&[root], None, None, None, false).unwrap();
        assert_eq!(result.file_count, 3);
    }

    #[test]
    fn pack_empty_dir_fails() {
        let d = TempDir::new().unwrap();
        let root = d.path();
        let result = pack_directories(&[root], None, None, None, false);
        assert!(result.is_err());
    }

    #[test]
    fn pack_not_a_dir_fails() {
        let d = TempDir::new().unwrap();
        let file = d.path().join("not_a_dir.txt");
        fs::write(&file, b"hello").unwrap();
        let result = pack_directories(&[&file], None, None, None, false);
        assert!(result.is_err());
    }

    #[test]
    fn default_output_name_single() {
        assert_eq!(default_output_name(&[PathBuf::from("C:/foo")]), "foo.txt");
    }

    #[test]
    fn default_output_name_multi_uses_first() {
        assert_eq!(
            default_output_name(&[PathBuf::from("a/one"), PathBuf::from("b/two")]),
            "one.txt"
        );
    }

    #[test]
    fn default_output_name_empty() {
        assert_eq!(default_output_name(&[]), "output.txt");
    }

    #[test]
    fn menu_location_single_dir_is_path_plus_txt() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("proj");
        fs::create_dir_all(&dir).unwrap();
        let (out, name) = menu_output_location(&[&dir], None);
        assert_eq!(name, "proj.txt");
        assert_eq!(out, d.path().to_path_buf());
    }

    #[test]
    fn menu_location_multi_uses_parent_name() {
        let d = setup_dir(&[("top/a.txt", b"1"), ("top/b.txt", b"2")]);
        let a = d.path().join("top/a.txt");
        let b = d.path().join("top/b.txt");
        let (out, name) = menu_output_location(&[&a, &b], None);
        assert_eq!(name, "top.txt");
        assert_eq!(out, d.path().join("top"));
    }

    #[test]
    fn menu_location_prefers_menu_dir() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("proj");
        let cwd = d.path().join("elsewhere");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let (out, name) = menu_output_location(&[&dir], Some(&cwd));
        assert_eq!(out, cwd);
        assert_eq!(name, "proj.txt");
    }

    #[test]
    fn auto_rename_when_free() {
        let d = TempDir::new().unwrap();
        let p = resolve_auto_rename(&d.path().join("foo.txt"));
        assert_eq!(p, d.path().join("foo.txt"));
    }

    #[test]
    fn auto_rename_when_exists() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("foo.txt"), b"x").unwrap();
        let p = resolve_auto_rename(&d.path().join("foo.txt"));
        assert_eq!(p, d.path().join("foo (1).txt"));
    }

    #[test]
    fn auto_rename_escalates() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("foo.txt"), b"x").unwrap();
        fs::write(d.path().join("foo (1).txt"), b"x").unwrap();
        let p = resolve_auto_rename(&d.path().join("foo.txt"));
        assert_eq!(p, d.path().join("foo (2).txt"));
    }

    #[test]
    fn pack_selected_roundtrip_byte_exact() {
        let d = setup_dir(&[("shared/a.txt", b"hello")]);
        let a = d.path().join("shared/a.txt");
        let result = pack_selected(&[&a], None, None, None, false).unwrap();
        assert_eq!(result.file_count, 1);
        let out = TempDir::new().unwrap();
        crate::unpack::unpack_to_dir(&result.container, out.path()).unwrap();
        let restored = fs::read(out.path().join("shared/a.txt")).unwrap();
        assert_eq!(restored, b"hello");
    }

    #[test]
    fn clipboard_mode_container_is_minimal() {
        let d = setup_dir(&[("a.txt", b"hello")]);
        let result = pack_selected(&[&d.path().join("a.txt")], None, None, None, true).unwrap();
        assert!(result.container.contains("X-Content-Length: 5"));
        assert!(!result.container.contains("X-Original-Charset"));
        assert!(!result.container.contains("X-Original-BOM"));
        assert!(!result.container.contains("X-Original-Size"));
        assert!(!result.container.contains("X-Original-SHA256"));
    }
}

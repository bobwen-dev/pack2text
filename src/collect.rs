use std::path::{Path, PathBuf};

use crate::encoding::{self, BomKind};
use crate::error::{Error, Result};

/// Default exclusion patterns applied when no --include is given.
///
/// Semantics: a bare name ("node_modules") matches a path component at any
/// depth; "*.ext" matches any component with that suffix; full-path globs
/// ("**/*.rs") match against the whole relative path. Hidden entries
/// (leading dot) are additionally skipped by the walker itself, so VCS
/// internals, IDE settings and dotenv-style secrets never leak by default.
const DEFAULT_EXCLUDES: &[&str] = &[
    // hidden files/dirs (belt-and-braces alongside the walker filter)
    ".*",
    // OS metadata (non-hidden)
    "Thumbs.db",
    "desktop.ini",
    "*.lnk",
    // editor swap/backup artifacts
    "*.swp",
    "*.swo",
    "*~",
    "*.orig",
    "*.rej",
    "*.bak",
    "*.tmp",
    "*.old",
    // dependency & build output directories
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".nox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".hypothesis",
    "htmlcov",
    "coverage",
    ".nyc_output",
    ".next",
    ".nuxt",
    ".output",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".vite",
    ".angular",
    ".expo",
    ".serverless",
    ".gradle",
    ".dart_tool",
    ".terraform",
    "Pods",
    "DerivedData",
    ".stack-work",
    "_build",
    "tmp",
    "temp",
    "logs",
    // lockfiles
    "*.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "packages.lock.json",
    // compiled artifacts & native libraries
    "*.exe",
    "*.dll",
    "*.so",
    "*.dylib",
    "*.a",
    "*.lib",
    "*.o",
    "*.obj",
    "*.rlib",
    "*.rmeta",
    "*.pdb",
    "*.class",
    "*.jar",
    "*.pyc",
    "*.pyo",
    "*.pyd",
    "*.wasm",
    "*.bin",
    "*.elf",
    "*.ko",
    // images (binary; SVG text stays included)
    "*.png",
    "*.jpg",
    "*.jpeg",
    "*.gif",
    "*.bmp",
    "*.tif",
    "*.tiff",
    "*.ico",
    "*.icns",
    "*.webp",
    "*.avif",
    "*.heic",
    "*.heif",
    "*.psd",
    "*.ai",
    "*.eps",
    "*.sketch",
    "*.fig",
    "*.xd",
    "*.svgz",
    // audio / video
    "*.mp3",
    "*.wav",
    "*.flac",
    "*.aac",
    "*.ogg",
    "*.oga",
    "*.opus",
    "*.m4a",
    "*.wma",
    "*.mid",
    "*.midi",
    "*.mp4",
    "*.mov",
    "*.avi",
    "*.mkv",
    "*.webm",
    "*.flv",
    "*.wmv",
    "*.m4v",
    "*.mpg",
    "*.mpeg",
    "*.3gp",
    // fonts
    "*.ttf",
    "*.otf",
    "*.woff",
    "*.woff2",
    "*.eot",
    // office / binary documents
    "*.pdf",
    "*.doc",
    "*.docx",
    "*.xls",
    "*.xlsx",
    "*.ppt",
    "*.pptx",
    "*.odt",
    "*.ods",
    "*.odp",
    "*.rtf",
    // archives / disk images / installers
    "*.zip",
    "*.tar",
    "*.gz",
    "*.tgz",
    "*.bz2",
    "*.tbz2",
    "*.xz",
    "*.txz",
    "*.7z",
    "*.rar",
    "*.zst",
    "*.tzst",
    "*.lz4",
    "*.br",
    "*.iso",
    "*.dmg",
    "*.img",
    "*.msi",
    "*.deb",
    "*.rpm",
    "*.apk",
    "*.appimage",
    "*.snap",
    // databases
    "*.db",
    "*.sqlite",
    "*.sqlite3",
    "*.mdb",
    "*.accdb",
    // logs
    "*.log",
    // secrets & certificates (never paste these into an AI chat)
    "*.pem",
    "*.key",
    "*.crt",
    "*.cer",
    "*.der",
    "*.pfx",
    "*.p12",
    "*.jks",
    "*.keystore",
    "*.ppk",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    // ML model weights (huge binaries)
    "*.onnx",
    "*.pt",
    "*.pth",
    "*.ckpt",
    "*.safetensors",
    "*.gguf",
    "*.pb",
    "*.h5",
    "*.tflite",
    "*.npy",
    "*.npz",
    "*.pkl",
    "*.parquet",
    "*.arrow",
    "*.feather",
    // minified code & source maps
    "*.min.js",
    "*.min.css",
    "*.map",
    // build metadata
    "*.tsbuildinfo",
    "*.egg-info",
    // project-specific
    "*.aloam",
];

pub struct CollectedFile {
    pub rel_path: String,
    pub original_charset: String,
    pub original_bom: BomKind,
    pub original_size: u64,
    pub original_sha256: String,
    pub utf8_content: String,
}

pub fn common_ancestor(selected: &[&Path]) -> PathBuf {
    if selected.is_empty() {
        return PathBuf::new();
    }
    let mut paths: Vec<Vec<std::path::Component>> = selected
        .iter()
        .map(|p| {
            let dir = if p.is_dir() {
                *p
            } else {
                p.parent().unwrap_or(p)
            };
            dir.components().collect()
        })
        .collect();
    let first = paths.remove(0);
    let mut common: Vec<std::path::Component> = Vec::new();
    for (i, comp) in first.iter().enumerate() {
        if paths.iter().all(|v| v.get(i) == Some(comp)) {
            common.push(*comp);
        } else {
            break;
        }
    }
    common.iter().collect()
}

struct CollectContext<'a> {
    root_name: String,
    include: Option<&'a [String]>,
    exclude: Option<&'a [String]>,
    ignores: ExcludeRules,
    defaults: ExcludeRules,
    skip_canonical: Option<PathBuf>,
}

impl CollectContext<'_> {
    fn try_push_file(
        &self,
        path: &Path,
        filter_rel: &str,
        display_rel: &str,
        ignores: &ExcludeRules,
        files: &mut Vec<CollectedFile>,
    ) -> Result<()> {
        if let Some(sc) = &self.skip_canonical
            && path.canonicalize().is_ok_and(|c| &c == sc)
        {
            return Ok(());
        }
        if ignores.is_excluded(filter_rel) {
            return Ok(());
        }
        if self.include.is_none() && self.defaults.is_excluded(filter_rel) {
            return Ok(());
        }
        if let Some(incs) = self.include
            && !incs.iter().any(|inc| glob_match(inc, filter_rel, false))
        {
            return Ok(());
        }
        if let Some(excs) = self.exclude
            && excs.iter().any(|exc| glob_match(exc, filter_rel, false))
        {
            return Ok(());
        }
        if display_rel.split(['/', '\\']).any(|seg| {
            seg.contains(':') || is_windows_reserved_name(seg) || seg.chars().count() > 255
        }) {
            eprintln!(
                "warning: skipping (path not portable to Windows): {}",
                path.display()
            );
            return Ok(());
        }

        let bytes = std::fs::read(path)?;
        match encoding::detect_and_convert(&bytes) {
            Some((charset, bom, utf8_content)) => {
                let original_size = bytes.len() as u64;
                let original_sha256 = crate::format::compute_sha256_hex(&bytes);
                files.push(CollectedFile {
                    rel_path: display_rel.to_string(),
                    original_charset: charset,
                    original_bom: bom,
                    original_size,
                    original_sha256,
                    utf8_content,
                });
            }
            None => {
                eprintln!(
                    "warning: skipping (binary/undetectable): {}",
                    path.display()
                );
            }
        }
        Ok(())
    }
}

fn build_context<'a>(
    root: &Path,
    include: Option<&'a [String]>,
    exclude: Option<&'a [String]>,
    skip_path: Option<&Path>,
) -> Result<CollectContext<'a>> {
    let skip_canonical = skip_path.and_then(|p| p.canonicalize().ok());
    let ignore_path = root.join(".ignore");
    let mut ignores = ExcludeRules::new();
    if ignore_path.exists() {
        ignores.load(&ignore_path);
    }
    let root_name = root
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .unwrap_or_else(|| "root".to_string());
    Ok(CollectContext {
        root_name,
        include,
        exclude,
        ignores,
        defaults: ExcludeRules::from_defaults(),
        skip_canonical,
    })
}

pub fn collect_text_files(
    root: &Path,
    include: Option<&[String]>,
    exclude: Option<&[String]>,
    skip_path: Option<&Path>,
) -> Result<Vec<CollectedFile>> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.display().to_string()));
    }

    let ctx = build_context(root, include, exclude, skip_path)?;
    let mut files = Vec::new();
    walk_files(root, "", &ctx.ignores, &ctx, &mut files)?;

    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(files)
}

/// Depth-first walk that layers each directory's `.ignore` onto the rules of
/// its ancestors and pops them again when leaving the subtree, so ignore
/// rules are scoped to the directory that declares them.
fn walk_files(
    root: &Path,
    prefix: &str,
    base_ignores: &ExcludeRules,
    ctx: &CollectContext,
    files: &mut Vec<CollectedFile>,
) -> Result<()> {
    let mut stack: Vec<(usize, ExcludeRules)> = vec![(0, base_ignores.clone())];
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.file_name()))
    {
        let entry = entry.map_err(|e| Error::Io(std::io::Error::other(e)))?;
        while stack.len() > 1 && stack.last().unwrap().0 >= entry.depth() {
            stack.pop();
        }

        if entry.file_type().is_dir() {
            if entry.depth() > 0 {
                let mut ignores = stack.last().unwrap().1.clone();
                let ignore_path = entry.path().join(".ignore");
                if ignore_path.exists() {
                    ignores.load(&ignore_path);
                }
                stack.push((entry.depth(), ignores));
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let ignores = &stack.last().unwrap().1;
        ctx.try_push_file(
            path,
            &format!("{prefix}{rel}"),
            &format!("{}/{prefix}{rel}", ctx.root_name),
            ignores,
            files,
        )?;
    }
    Ok(())
}

pub fn collect_selected(
    selected: &[&Path],
    include: Option<&[String]>,
    exclude: Option<&[String]>,
    skip_path: Option<&Path>,
) -> Result<Vec<CollectedFile>> {
    if selected.is_empty() {
        return Err(Error::NoTextFiles);
    }
    let ancestor = common_ancestor(selected);
    let ctx = build_context(&ancestor, include, exclude, skip_path)?;
    let mut files = Vec::new();

    for sel in selected {
        let rel_sel = sel
            .strip_prefix(&ancestor)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        if sel.is_dir() {
            let mut sel_ignores = ctx.ignores.clone();
            if sel != &ancestor {
                let sel_ignore = sel.join(".ignore");
                if sel_ignore.exists() {
                    sel_ignores.load(&sel_ignore);
                }
            }
            let prefix = if rel_sel.is_empty() {
                String::new()
            } else {
                format!("{rel_sel}/")
            };
            walk_files(sel, &prefix, &sel_ignores, &ctx, &mut files)?;
        } else if sel.is_file() {
            let mut file_ignores = ctx.ignores.clone();
            let mut dir = sel.parent();
            while let Some(d) = dir {
                if d == ancestor || !d.starts_with(&ancestor) {
                    break;
                }
                let ig = d.join(".ignore");
                if ig.exists() {
                    file_ignores.load(&ig);
                }
                dir = d.parent();
            }
            let display_rel = format!("{}/{rel_sel}", ctx.root_name);
            ctx.try_push_file(sel, &rel_sel, &display_rel, &file_ignores, &mut files)?;
        } else {
            eprintln!(
                "warning: skipping (neither file nor dir): {}",
                sel.display()
            );
        }
    }

    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(files)
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|s| s.starts_with('.'))
}

/// Windows reserves these device names for any extension (CON, con.txt, ...).
pub(crate) fn is_windows_reserved_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[derive(Clone)]
struct ExcludeRule {
    pattern: String,
    negated: bool,
    directory_only: bool,
}

#[derive(Clone)]
struct ExcludeRules {
    rules: Vec<ExcludeRule>,
}

impl ExcludeRules {
    fn new() -> Self {
        ExcludeRules { rules: Vec::new() }
    }

    fn from_defaults() -> Self {
        let mut rules = ExcludeRules::new();
        for &p in DEFAULT_EXCLUDES {
            rules.add(p);
        }
        rules
    }

    fn add(&mut self, pattern: &str) {
        let negated = pattern.starts_with('!');
        let s = if negated { &pattern[1..] } else { pattern };
        let directory_only = s.ends_with('/');
        let s = if directory_only { &s[..s.len() - 1] } else { s };
        self.rules.push(ExcludeRule {
            pattern: s.to_string(),
            negated,
            directory_only,
        });
    }

    fn load(&mut self, path: &Path) {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                self.add(line);
            }
        }
    }

    fn is_excluded(&self, rel_path: &str) -> bool {
        let path = rel_path.replace('\\', "/");
        let mut last_matched_negated = false;
        let mut any_positive = false;

        for rule in &self.rules {
            if glob_match(&rule.pattern, &path, rule.directory_only) {
                if rule.negated {
                    if any_positive {
                        last_matched_negated = true;
                    }
                } else {
                    any_positive = true;
                    last_matched_negated = false;
                }
            }
        }

        any_positive && !last_matched_negated
    }
}

fn glob_match(pattern: &str, path: &str, directory_only: bool) -> bool {
    if pattern.is_empty() {
        return false;
    }

    if directory_only {
        return path
            .split('/')
            .any(|component| glob_match_inner(pattern, component));
    }

    if glob_match_inner(pattern, path) {
        return true;
    }
    path.split('/')
        .any(|component| glob_match_inner(pattern, component))
}

#[derive(Clone, Copy, PartialEq)]
enum GlobPat {
    /// Single `*`: matches any run of characters except `/`.
    Star,
    /// `**/` as a unit: matches zero or more complete directory segments,
    /// each ending in `/`, or nothing (so it may span the whole tree).
    DStarSlash,
    /// Trailing `**`: matches any remaining characters including `/`.
    DStarTail,
    /// `?`: matches exactly one character except `/`.
    Q,
    /// A literal character.
    Lit(char),
}

fn tokenize_glob(pattern: &str) -> Vec<GlobPat> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    out.push(GlobPat::DStarSlash);
                    i += 3;
                } else {
                    out.push(GlobPat::DStarTail);
                    i += 2;
                }
            }
            '*' => {
                out.push(GlobPat::Star);
                i += 1;
            }
            '?' => {
                out.push(GlobPat::Q);
                i += 1;
            }
            c => {
                out.push(GlobPat::Lit(c));
                i += 1;
            }
        }
    }
    out
}

fn glob_match_inner(pattern: &str, text: &str) -> bool {
    let p = tokenize_glob(pattern);
    let s: Vec<char> = text.chars().collect();
    let (pl, sl) = (p.len(), s.len());
    let mut dp = vec![vec![false; sl + 1]; pl + 1];
    let mut mid = vec![vec![false; sl + 1]; pl + 1];
    dp[0][0] = true;
    for i in 1..=pl {
        dp[i][0] = matches!(
            p[i - 1],
            GlobPat::Star | GlobPat::DStarSlash | GlobPat::DStarTail
        ) && dp[i - 1][0];
    }
    for i in 1..=pl {
        for j in 1..=sl {
            dp[i][j] = match p[i - 1] {
                GlobPat::Star => dp[i - 1][j] || (s[j - 1] != '/' && dp[i][j - 1]),
                GlobPat::DStarSlash => {
                    mid[i][j] = s[j - 1] != '/' && (mid[i][j - 1] || dp[i - 1][j - 1]);
                    dp[i - 1][j] || (s[j - 1] == '/' && mid[i][j - 1])
                }
                GlobPat::DStarTail => dp[i - 1][j] || dp[i][j - 1],
                GlobPat::Q => dp[i - 1][j - 1] && s[j - 1] != '/',
                GlobPat::Lit(c) => dp[i - 1][j - 1] && c == s[j - 1],
            };
        }
    }
    dp[pl][sl]
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
    fn collect_text_only() {
        let d = setup_dir(&[("hello.rs", b"fn main() {}"), ("lib.rs", b"pub fn x() {}")]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn skips_binary() {
        let d = setup_dir(&[
            ("code.rs", b"fn main() {}"),
            ("binary.bin", &[0x00, 0x01, 0x02]),
        ]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.contains("code.rs"));
    }

    #[test]
    fn respects_ignore() {
        let d = setup_dir(&[
            ("src/main.rs", b"code"),
            ("src/secret.rs", b"hidden"),
            (".ignore", b"secret.rs"),
        ]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn empty_dir() {
        let d = TempDir::new().unwrap();
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn skips_hidden() {
        let d = setup_dir(&[("visible.rs", b"code"), (".hidden", b"hidden")]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn glob_match_basic() {
        assert!(glob_match_inner("*.rs", "main.rs"));
        assert!(!glob_match_inner("*.rs", "main.py"));
        assert!(glob_match_inner("test", "test"));
        assert!(glob_match_inner("*", "anything"));
    }

    #[test]
    fn glob_match_slash_boundaries() {
        assert!(glob_match("a/*/c", "a/b/c", false));
        assert!(!glob_match("a/*/c", "a/x/y/c", false));
        assert!(!glob_match("a/*/c", "a/b/c/d", false));
        assert!(glob_match("a?c", "abc", false));
        assert!(!glob_match("a?c", "a/c", false));
        assert!(glob_match("**/*.rs", "src/main.rs", false));
        assert!(glob_match("**/*.rs", "a.rs", false));
        assert!(glob_match("a/**/b", "a/b", false));
        assert!(glob_match("a/**/b", "a/x/b", false));
        assert!(!glob_match("a/**/b", "a/xb", false));
        assert!(glob_match("a/**", "a/x/b", false));
        assert!(glob_match("a/**", "a/x", false));
        assert!(glob_match("**", "a/b/c", false));
        assert!(glob_match("*c", "abc", false));
        assert!(glob_match("*c", "a/bc", false));
        assert!(glob_match_inner("*c", "bc"));
        assert!(!glob_match_inner("*c", "a/bc"));
    }

    #[test]
    fn rel_path_prefixed_with_root_name() {
        let base = TempDir::new().unwrap();
        let root = base.path().join("foo");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("foobar.txt"), b"x").unwrap();
        let files = collect_text_files(&root, None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].rel_path, "foo/foobar.txt");
    }

    #[test]
    fn collects_utf16_file() {
        let mut content = vec![0xFF, 0xFE];
        for ch in "hello".encode_utf16() {
            content.extend_from_slice(&ch.to_le_bytes());
        }
        let d = setup_dir(&[("utf16.txt", &content)]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].original_charset, "utf-16le");
        assert_eq!(files[0].utf8_content, "hello");
    }

    #[test]
    fn include_filter_keeps_matching() {
        let d = setup_dir(&[("a.rs", b"1"), ("b.py", b"2"), ("c.rs", b"3")]);
        let files =
            collect_text_files(d.path(), Some(&[String::from("*.rs")]), None, None).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.rel_path.ends_with(".rs")));
    }

    #[test]
    fn exclude_filter_drops_matching() {
        let d = setup_dir(&[("a.rs", b"1"), ("b.py", b"2"), ("c.rs", b"3")]);
        let files =
            collect_text_files(d.path(), None, Some(&[String::from("*.rs")]), None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with(".py"));
    }

    #[test]
    fn include_bypasses_default_excludes() {
        let d = setup_dir(&[("Cargo.lock", b"lock content"), ("b.txt", b"2")]);
        let files =
            collect_text_files(d.path(), Some(&[String::from("Cargo.lock")]), None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("Cargo.lock"));
    }

    #[test]
    fn default_excludes_still_apply_without_include() {
        let d = setup_dir(&[("Cargo.lock", b"lock content"), ("b.txt", b"2")]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with(".txt"));
    }

    #[test]
    fn default_excludes_cover_common_noise() {
        let d = setup_dir(&[
            ("main.rs", b"keep"),
            ("Cargo.lock", b"lock"),
            ("yarn.lock", b"lock"),
            ("app.min.js", b"minified"),
            ("bundle.map", b"sourcemap"),
            ("model.safetensors", b"weights"),
            ("server.pem", b"secret"),
            ("id_rsa", b"key"),
            ("backup.aloam", b"custom"),
            ("photo.png", b"img"),
            ("clip.mp4", b"vid"),
            ("font.ttf", b"font"),
            ("doc.pdf", b"doc"),
            ("archive.zip", b"zip"),
            ("data.sqlite", b"db"),
            ("debug.log", b"log"),
            ("lib.o", b"obj"),
            ("notes.bak", b"bak"),
            ("node_modules/x.js", b"dep"),
            ("target/debug/x.rs", b"artifact"),
            ("dist/app.js", b"built"),
            (".venv/y.py", b"env"),
        ]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("main.rs"));
    }

    #[test]
    fn include_still_respects_ignore_file() {
        let d = setup_dir(&[("a.rs", b"1"), ("b.rs", b"2"), (".ignore", b"b.rs")]);
        let files =
            collect_text_files(d.path(), Some(&[String::from("*.rs")]), None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("a.rs"));
    }

    #[test]
    fn common_ancestor_mixed() {
        let d = TempDir::new().unwrap();
        let a = d.path().join("top/a");
        let b = d.path().join("top/b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let anc = common_ancestor(&[&a, &b]);
        assert_eq!(anc, d.path().join("top"));
    }

    #[test]
    fn common_ancestor_single_dir_is_itself() {
        let d = TempDir::new().unwrap();
        let a = d.path().join("top/a");
        fs::create_dir_all(&a).unwrap();
        assert_eq!(common_ancestor(&[&a]), a);
    }

    #[test]
    fn common_ancestor_file_uses_parent() {
        let d = setup_dir(&[("top/a.txt", b"1"), ("top/b.txt", b"2")]);
        let a = d.path().join("top/a.txt");
        let b = d.path().join("top/b.txt");
        assert_eq!(common_ancestor(&[&a, &b]), d.path().join("top"));
    }

    #[test]
    fn collect_selected_only_selected_items() {
        let d = setup_dir(&[
            ("shared/a.txt", b"aaa"),
            ("shared/b.txt", b"bbb"),
            ("shared/c.txt", b"ccc"),
        ]);
        let a = d.path().join("shared/a.txt");
        let b = d.path().join("shared/b.txt");
        let files = collect_selected(&[&a, &b], None, None, None).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.rel_path.ends_with("a.txt")));
        assert!(files.iter().any(|f| f.rel_path.ends_with("b.txt")));
        assert!(!files.iter().any(|f| f.rel_path.ends_with("c.txt")));
    }

    #[test]
    fn collect_selected_single_dir_rel_prefixed() {
        let d = setup_dir(&[
            ("proj/src/main.rs", b"fn main() {}"),
            ("proj/README.md", b"hi"),
        ]);
        let proj = d.path().join("proj");
        let files = collect_selected(&[&proj], None, None, None).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.rel_path.starts_with("proj/")));
    }

    #[test]
    fn collect_selected_mixed_dir_and_file() {
        let d = setup_dir(&[
            ("top/a/x.txt", b"1"),
            ("top/b.txt", b"2"),
            ("top/unrelated.txt", b"3"),
        ]);
        let a = d.path().join("top/a");
        let b = d.path().join("top/b.txt");
        let files = collect_selected(&[&a, &b], None, None, None).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.rel_path.ends_with("x.txt")));
        assert!(files.iter().any(|f| f.rel_path.ends_with("b.txt")));
        assert!(!files.iter().any(|f| f.rel_path.ends_with("unrelated.txt")));
    }

    #[test]
    fn nested_ignore_applies_in_subdir() {
        let d = setup_dir(&[
            ("src/main.rs", b"code"),
            ("src/secret.rs", b"hidden"),
            ("src/.ignore", b"secret.rs"),
        ]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("main.rs"));
    }

    #[test]
    fn nested_ignore_scoped_to_its_subtree() {
        let d = setup_dir(&[
            ("src/a/secret.rs", b"hidden"),
            ("src/.ignore", b"secret.rs"),
            ("other/secret.rs", b"visible elsewhere"),
        ]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("other/secret.rs"));
    }

    #[test]
    fn include_multiple_patterns_any_match() {
        let d = setup_dir(&[("a.rs", b"1"), ("b.py", b"2"), ("c.txt", b"3")]);
        let incs = [String::from("*.rs"), String::from("*.py")];
        let files = collect_text_files(d.path(), Some(&incs), None, None).unwrap();
        assert_eq!(files.len(), 2);
        assert!(
            files
                .iter()
                .all(|f| f.rel_path.ends_with(".rs") || f.rel_path.ends_with(".py"))
        );
    }

    #[test]
    fn exclude_multiple_patterns_any_match() {
        let d = setup_dir(&[("a.rs", b"1"), ("b.py", b"2"), ("c.txt", b"3")]);
        let excs = [String::from("*.rs"), String::from("*.py")];
        let files = collect_text_files(d.path(), None, Some(&excs), None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with(".txt"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_colon_in_rel_path() {
        let d = setup_dir(&[("a.txt", b"ok"), ("x:y.txt", b"colon")]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("a.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_windows_reserved_name() {
        let d = setup_dir(&[("a.txt", b"ok"), ("con.txt", b"reserved")]);
        let files = collect_text_files(d.path(), None, None, None).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].rel_path.ends_with("a.txt"));
    }

    #[test]
    fn collect_selected_respects_selected_dir_ignore() {
        let d = setup_dir(&[
            ("top/sub/secret.txt", b"keep out"),
            ("top/sub/ok.txt", b"fine"),
            ("top/sibling.txt", b"also selected"),
        ]);
        fs::write(d.path().join("top/sub/.ignore"), b"secret.txt").unwrap();
        let sub = d.path().join("top/sub");
        let sibling = d.path().join("top/sibling.txt");
        let files = collect_selected(&[&sub, &sibling], None, None, None).unwrap();
        assert!(files.iter().any(|f| f.rel_path.ends_with("ok.txt")));
        assert!(files.iter().any(|f| f.rel_path.ends_with("sibling.txt")));
        assert!(!files.iter().any(|f| f.rel_path.ends_with("secret.txt")));
    }

    #[test]
    fn collect_selected_file_respects_parent_ignore() {
        let d = setup_dir(&[
            ("top/sub/keep.txt", b"1"),
            ("top/sub/secret.txt", b"2"),
            ("top/sibling.txt", b"3"),
        ]);
        fs::write(d.path().join("top/sub/.ignore"), b"secret.txt").unwrap();
        let keep = d.path().join("top/sub/keep.txt");
        let secret = d.path().join("top/sub/secret.txt");
        let sibling = d.path().join("top/sibling.txt");
        let files = collect_selected(&[&keep, &secret, &sibling], None, None, None).unwrap();
        assert!(files.iter().any(|f| f.rel_path.ends_with("keep.txt")));
        assert!(files.iter().any(|f| f.rel_path.ends_with("sibling.txt")));
        assert!(!files.iter().any(|f| f.rel_path.ends_with("secret.txt")));
    }
}

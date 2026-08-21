<!--
name: pack2text
version: 0.1.0
lang: Rust
edition: 2024
crate-type: binary
keywords: txt, pack, unpack, codebase, ai-context, deepseek, chatgpt, claude, clipboard
-->

# pack2text

**Copy an entire directory's code and text to the clipboard in one click — or save it as a single, readable, byte-exact reversible `.txt`.**

`pack2text` puts any directory — or a multi-selected set of files — into your clipboard as one readable text bundle: the fastest way to hand a whole codebase to an AI. The same bundle can also be written as a single `.txt` container you can read, diff, attach, or archive — and unpack restores every file **byte-for-byte**, including original encoding, BOM, and paths.

- [Features](#features)
- [Quick Start](#quick-start)
- [Why](#why)
- [Usage](#usage)
- [Multi-select Naming](#multi-select-naming)
- [Container Format](#container-format)
- [Exclude Rules](#exclude-rules)
- [Building](#building)
- [Known Limits](#known-limits)
- [Related](#related)

---

## Features

- **Copy directory to clipboard** — right-click a folder (or multi-select files), hit "Pack to Clipboard", paste straight into any AI chat. No file created, nothing to manage.
- **Explorer / Finder context menu** — right-click directories *or* files (multi-select supported):
  - **Windows**: registry entries under `HKCU`, no admin rights
  - **macOS**: Automator Quick Actions (`~/Library/Services/*.workflow`)
  - **Pack to Clipboard** — the whole selection becomes clipboard text (primary)
  - **Pack to Text** — optionally writes `<selection>.txt` into the current Explorer folder, auto-renaming (`name (1).txt`) when the target exists — never overwrites
- **Byte-exact roundtrip** — every file's original encoding, BOM, and SHA-256 are recorded in the container; unpack re-encodes and verifies every byte, so you get the originals back exactly or a loud, specific error.
- **Multi-encoding detection** — UTF-8, UTF-16 LE/BE (BOM), GB18030, Shift_JIS, EUC-KR, EUC-JP, Windows-1252. Binary or undetectable content is skipped with a warning — never silently corrupted.
- **`.ignore` rules** — gitignore-style, loaded recursively per-directory and scoped to the directory that declares them (with `!` re-inclusion). Sensible built-in defaults for build dirs, lock files, archives, and binaries.
- **Glob filters** — repeatable `--include` / `--exclude` with full `**` semantics (`a/**/b`, `a/**`, `**/*.rs`).
- **Safe by design** — path traversal, duplicate paths, reserved Windows names, over-long components, and overwriting existing files are rejected before anything is written; extraction is all-or-nothing with rollback on any write failure.
- **Human-readable format** — a multipart text file you can read, diff, and hand-edit; parsing accepts case-insensitive headers and quoted or unquoted filenames.
- **Single binary** — no runtime, no installer, no external files. Transactional `--install-menu` / `--uninstall-menu` (per-user, no admin rights).
- **Error visibility** — when launched from Explorer there is no console; errors surface in a message box instead of vanishing.

## Quick Start

### Windows

```powershell
# 1. Install the context menu (run once)
pack2text.exe --install-menu

# 2. In Explorer, right-click a folder (or select several files) →
#    "Pack to Clipboard" → paste into your AI chat. Done.
```

Want the bundle as a file instead? Pick **Pack to Text**: it writes `SelectionName.txt` into the folder you are viewing, auto-renaming (`SelectionName (1).txt`) on repeat — existing files are never touched.

### macOS

> **Not tested on a real Mac.** The Quick Action bundles are cross-compile-verified and structurally validated, but the Finder integration itself has not been exercised on macOS hardware. Expect to be the first to run it.

```bash
# 1. Install the Finder Quick Actions (run once)
pack2text --install-menu

# 2. In Finder, right-click a folder (or select several files) →
#    Quick Actions → Pack to Clipboard / Pack to Text
```

If the actions don't appear immediately, enable them in **System Settings → Extensions → Finder → Quick Actions** (or run `killall Finder` once). Remove with `pack2text --uninstall-menu`.

## Why

AI assistants read pasted text natively, but they can't read an entire project in one shot — you either paste files one by one or zip them, which most chat UIs can't read at all. For humans, a codebase that lives in plain text can be diffed, grepped, and pasted anywhere.

`pack2text` converts any selection — a whole directory, or a few files picked across folders — into one readable text bundle that goes **straight to your clipboard**: no file, no intermediate step, just select → copy → paste. Every file becomes a marked section with its relative path in the header, ready for the AI to read, search, and reason about your project as a whole. The same bundle can optionally be saved as a `.txt` container — readable, diffable, attachable — and unpack restores the originals **byte-for-byte**, original encoding, BOM, and directory layout included.

## Usage

```
Usage: pack2text [OPTIONS] [DIRECTORIES]...

Arguments:
  [DIRECTORIES]...  Directories or files to pack

Options:
  -o, --output <OUTPUT>        Output file path
  -c, --clipboard              Output to clipboard instead of file
  -f, --force                  Overwrite existing output file
      --menu                   Context-menu mode (shared parent root, auto-rename)
      --menu-dir <MENU_DIR>    Context-menu current directory (Explorer %V)
      --unpack <UNPACK>        Unpack container file to directory
      --unpack-dir <UNPACK_DIR>  Output directory for --unpack (default: unpacked)
      --install-menu           Install Explorer/Finder context menu entries
      --uninstall-menu         Uninstall Explorer/Finder context menu entries
      --include <INCLUDE>      Include files matching glob (repeatable, any match wins)
      --exclude <EXCLUDE>      Exclude files matching glob (repeatable, any match wins)
  -v, --verbose                Verbose output
```

### Examples

```powershell
# Pack a project into a container, then verify by unpacking
pack2text C:\src\myproject -o myproject.txt
pack2text --unpack myproject.txt --unpack-dir restored
# → "extracted 12 files to restored" — byte-identical to the source

# Only Rust and Python sources, from several folders at once
pack2text a b --include "*.rs" --include "*.py"

# No file — straight to the clipboard
pack2text C:\src\myproject --clipboard
```

## Multi-select Naming

| Selection | Container name | Location |
|---|---|---|
| Single directory `foo` | `foo.txt` | next to `foo` (or current Explorer folder) |
| Multiple items | `<parent folder name>.txt` | current Explorer folder |
| Any (CLI `-o`) | your choice | your choice |

## Container Format

A multipart text file — readable by eye, strict by machine:

```
--=pack2text_a1b2c3d4e5f60718293a4b5c6d7e8f9=--
Content-Disposition: form-data; name="file"; filename="src/main.rs"
Content-Type: text/plain; charset=utf-8
X-Original-Charset: utf-8
X-Original-BOM: none
X-Original-Size: 246
X-Original-SHA256: 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
X-Content-Length: 246

fn main() { ... }
--=pack2text_a1b2c3d4e5f60718293a4b5c6d7e8f9=--
```

- Header names are case-insensitive; `filename=` accepts quoted or unquoted values (hand-edited containers parse cleanly)
- Bodies are delimited by declared length, so container text inside a file cannot break parsing
- Unpack **refuses** on: path traversal, duplicate paths, size or SHA-256 mismatch, unknown charset/BOM, existing files, non-portable paths (control chars, `"<>|?*`, reserved device names like `con.txt`, colon components, >255-char components)

## Exclude Rules

Create a `.ignore` file (gitignore syntax) — rules are loaded recursively and scoped to the directory that declares them:

```gitignore
# .ignore
*.log
!important.log   # re-include this one
build/           # ignore the whole build dir
secret-*.txt
```

If no `.ignore` exists, defaults apply: dotfiles, `node_modules`, `target`, `dist`, `build`, lock files, archives, and binary formats are excluded.

## Building

Requires Rust (edition 2024):

```bash
# Windows exe (GNU toolchain)
cargo build --release --target x86_64-pc-windows-gnu

# macOS
cargo build --release --target x86_64-apple-darwin

# Linux (CLI works everywhere; context menu is Windows/macOS-only)
cargo build --release
```

```bash
cargo test        # 100+ unit & property tests
cargo clippy --all-targets -- -D warnings
```

## Known Limits

- **Encoding ambiguity is inherent**: a GBK byte sequence that happens to be valid UTF-8 is detected as UTF-8. Round-trips stay byte-exact in every case; the *displayed* text may differ from intent in ambiguous cases.
- Binary and undetectable content is skipped with a warning — this is a text-packing tool.
- **macOS integration is not tested on a real Mac.** The Quick Action bundles are cross-compile-verified (`cargo check --target x86_64-apple-darwin`) and the generated plists validate with `plistlib`, but no macOS hardware has run them. Treat the Finder menu as experimental until verified.
- Context-menu integration, clipboard, and message boxes are verified by cross-compilation and registration inspection; a final sanity pass on real Windows/macOS desktops is recommended after first install.

## Related

Prefer a **PDF** with embedded images, or a **DOCX**? Same idea, different containers:

- [pack2pdf](https://github.com/bobwen-dev/pack2pdf) — pack a project into one page-numbered PDF with embedded images and CJK support
- [docpack](https://github.com/bobwen-dev/docpack) — pack a project into one DOCX with a GUI and i18n

## License

MIT — see [LICENSE](LICENSE).

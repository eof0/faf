# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Changed

- **Performance** - further hot-path trimming without changing match semantics:
  - Raw reverse-byte scan for filename extraction on non-root entries, bypassing `Path::file_name()`'s `Components` parsing
  - Filename stem computed once per entry in default (stem) match mode, shared between the length prefilter and the equality check
  - Substring color-highlight path now reads `MatchTarget`'s cached bytes/ASCII flag instead of rescanning the query string per match
- Removed duplicate query-string storage (`WalkConfig::target_raw`/`target_canonical`) - `MatchTarget` already held the same bytes for substring matching

## [1.1.0] - 2026-05-16

### Added

- **`faf` binary** - a shorter command name alongside `fafind`. Both binaries run the same program; install or symlink either (or both). The summary line uses the name you invoked (`faf:` vs `fafind:`).
- **Expanded terminal color highlighting** when stdout is a TTY (see below). Disabled automatically for `-0` / `--null`, or override with `--color always|never|auto`.

#### Color highlighting

Colors apply to the **filename** portion of each match line (the path prefix before the final `/` is dimmed). Extensions are everything from the last `.` in the basename (same rule as stem matching).

| Color | When | What it highlights |
|-------|------|-------------------|
| **Dim** | All modes | Directory path before the filename (`/path/to/`) |
| **Green** | Default (stem), `-p` (exact), `-s` (substr) | The matching part of the name - full stem in default/`-p`, each substring hit in `-s` |
| **Bold + green** | `-p` (exact) | The filename stem (name without extension) |
| **Yellow** | Default, `-p`, `-s` | File extension (`.rs`, `.js`, `.docx`, etc.) |
| **Orange** | `-s` (substr) only | Non-matching parts of the filename stem (before/after/between matches) |

**Default (stem) example** - `faf main .` matching `main.rs`:

- Dim: `/projects/app/`
- Green: `main`
- Yellow: `.rs`

**Substring example** - `faf -s main .` matching `has_dot_entry_main_corner.js`:

- Dim: `/this/folder/`
- Orange: `has_dot_entry_`
- Green: `main`
- Orange: `_corner`
- Yellow: `.js`

Case-insensitive `-s` with non-ASCII names falls back to plain output (no orange/green split) to avoid incorrect highlighting.

### Changed

- **Output path** - matches stream to stdout as they are found. Piped output (`stdout` not a TTY) flushes after each match line so downstream tools see results immediately; interactive terminals batch writes to reduce lock contention.
- **Performance** - several hot-path improvements without changing match semantics:
  - Per-worker output batching on TTY (64 KiB threshold before taking the stdout lock)
  - Atomic scan/match counters instead of a mutex on the hot path
  - Cached match config per worker thread (fewer `Arc` dereferences in `process_entry`)
  - Length prefilter in the matcher before full compare / substring search

### Packaging note

Release tarballs ship one `fafind` binary; AUR/Homebrew install `faf` as a symlink. See `packaging/aur/README.md` for the AUR release checklist.

---

## [1.0.1] - 2025-03-XX

### Added

- Initial colorized match highlighting
- `--color auto|always|never` flag

---

## [1.0.0]

### Added

- Initial release - parallel filename search with stem, substring (`-s`), and exact (`-p`) modes

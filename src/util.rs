use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::config::{MatchMode, WalkConfig};
use crate::matcher::{MatchTarget, NAME_BUF, ascii_lower, stem_bytes};

const GREEN: &[u8] = b"\x1b[32m";
const BOLD_GREEN: &[u8] = b"\x1b[1;32m";
const DIM: &[u8] = b"\x1b[2m";
const YELLOW: &[u8] = b"\x1b[33m";
/// Non-matching filename segments in substring mode.
const ORANGE: &[u8] = b"\x1b[38;5;208m";
const RESET: &[u8] = b"\x1b[0m";

/// Final path component of a walk entry.
///
/// Skips `Path::file_name()` and its `Components` normalization. Entry
/// names come straight from `readdir`, so they are never `.`, `..` or
/// slash-terminated. Only the walk root needs the general parser.
#[inline(always)]
pub fn entry_file_name_bytes(path: &Path) -> &[u8] {
    let full = path.as_os_str().as_bytes();
    match memchr::memrchr(b'/', full) {
        Some(i) => &full[i + 1..],
        None => full,
    }
}

#[inline(always)]
pub fn append_path(buf: &mut Vec<u8>, path: &Path, null_terminate: bool) {
    buf.extend_from_slice(path.as_os_str().as_bytes());
    buf.push(if null_terminate { b'\0' } else { b'\n' });
}

/// `name` is the final component of `path`, already extracted by the caller.
#[inline(always)]
pub fn append_path_highlight(buf: &mut Vec<u8>, path: &Path, name: &[u8], cfg: &WalkConfig) {
    let full = path.as_os_str().as_bytes();
    paint(buf, DIM, &full[..full.len() - name.len()]);
    match cfg.target.mode {
        MatchMode::Substr => highlight_substr(buf, name, &cfg.target),
        MatchMode::Precise | MatchMode::Standard => {
            let stem = stem_bytes(name).len();
            let color = if cfg.target.mode == MatchMode::Precise {
                BOLD_GREEN
            } else {
                GREEN
            };
            paint(buf, color, &name[..stem]);
            paint(buf, YELLOW, &name[stem..]);
        }
    }
    buf.push(b'\n');
}

#[inline(always)]
fn paint(buf: &mut Vec<u8>, color: &[u8], text: &[u8]) {
    if text.is_empty() {
        return;
    }
    buf.extend_from_slice(color);
    buf.extend_from_slice(text);
    buf.extend_from_slice(RESET);
}

/// Highlights land on the real hit, including hits inside the extension.
/// ASCII folding keeps byte positions, so it is applied to any name when
/// the query is ASCII. Unicode folding does not, so those get no inner
/// highlight.
fn highlight_substr(buf: &mut Vec<u8>, name: &[u8], target: &MatchTarget) {
    let mut lower = [0u8; NAME_BUF];
    let hay = if !target.ignore_case {
        Some(name)
    } else if target.ascii {
        ascii_lower(name, &mut lower)
    } else {
        None
    };
    let Some(hay) = hay else {
        return paint(buf, ORANGE, name);
    };
    let n = target.needle().len();
    let mut last = 0;
    for at in target.finder.find_iter(hay) {
        paint(buf, ORANGE, &name[last..at]);
        paint(buf, GREEN, &name[at..at + n]);
        last = at + n;
    }
    paint(buf, ORANGE, &name[last..]);
}

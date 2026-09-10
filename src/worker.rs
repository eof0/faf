use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::CacheWriter;
use crate::config::{EntryType, WalkConfig};
use crate::util::{append_path, append_path_highlight, entry_file_name_bytes};

const OUT_BUF_CAP: usize = 256 * 1024;
const FLUSH_THRESHOLD: usize = 64 * 1024;

pub struct Totals {
    scanned: AtomicU64,
    found: AtomicU64,
}

impl Totals {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            scanned: AtomicU64::new(0),
            found: AtomicU64::new(0),
        })
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.scanned.load(Ordering::Relaxed),
            self.found.load(Ordering::Relaxed),
        )
    }
}

pub struct WorkerState {
    pub config: Arc<WalkConfig>,
    totals: Arc<Totals>,
    cache_writer: Option<Arc<CacheWriter>>,
    scanned: u64,
    found: u64,
    out_buf: Vec<u8>,
    cache_buf: Vec<u8>,
}

impl WorkerState {
    pub fn new(
        config: Arc<WalkConfig>,
        totals: Arc<Totals>,
        cache_writer: Option<Arc<CacheWriter>>,
    ) -> Self {
        Self {
            config,
            totals,
            cache_writer,
            scanned: 0,
            found: 0,
            out_buf: Vec::with_capacity(OUT_BUF_CAP),
            cache_buf: Vec::new(),
        }
    }
}

impl Drop for WorkerState {
    fn drop(&mut self) {
        flush_stdout(&mut self.out_buf);
        if let Some(writer) = &self.cache_writer {
            writer.write(&self.cache_buf);
        }
        self.totals
            .scanned
            .fetch_add(self.scanned, Ordering::Relaxed);
        self.totals.found.fetch_add(self.found, Ordering::Relaxed);
    }
}

/// False once stdout is gone, e.g. the reader closed the pipe.
#[inline(never)]
fn flush_stdout(buf: &mut Vec<u8>) -> bool {
    if buf.is_empty() {
        return true;
    }
    let ok = std::io::stdout().lock().write_all(buf).is_ok();
    buf.clear();
    ok
}

/// Hot path, called for every entry. Returns false when the walk should stop.
///
/// The root path comes from the user and may be `.`, `/` or slash-terminated,
/// so it takes the general `Path::file_name()` route. Every other entry name
/// comes from `readdir` and is read with a single reverse byte scan.
#[inline(always)]
pub fn process_entry(path: &Path, is_dir: bool, is_root: bool, state: &mut WorkerState) -> bool {
    let cfg = &*state.config;
    state.scanned += 1;

    match cfg.entry_type {
        EntryType::File if is_dir => return true,
        EntryType::Dir if !is_dir => return true,
        _ => {}
    }

    let name = if is_root {
        match path.file_name() {
            Some(f) => f.as_bytes(),
            None => return true,
        }
    } else {
        entry_file_name_bytes(path)
    };

    if cfg.verbose {
        verbose_scan(path);
    }
    if !cfg.target.is_match(name) {
        return true;
    }
    state.found += 1;

    if let Some(writer) = &state.cache_writer {
        append_path(&mut state.cache_buf, path, true);
        if state.cache_buf.len() >= FLUSH_THRESHOLD {
            writer.write(&state.cache_buf);
            state.cache_buf.clear();
        }
    }

    if cfg.verbose {
        state
            .out_buf
            .extend_from_slice(format!("[MATCH] {}\n", path.display()).as_bytes());
    } else if cfg.color {
        append_path_highlight(&mut state.out_buf, path, name, cfg);
    } else {
        append_path(&mut state.out_buf, path, cfg.null_terminate);
    }
    state.out_buf.len() < FLUSH_THRESHOLD || flush_stdout(&mut state.out_buf)
}

#[cold]
#[inline(never)]
fn verbose_scan(path: &Path) {
    eprintln!("[SCAN] {}", path.display());
}

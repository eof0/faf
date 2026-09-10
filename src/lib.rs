#[cfg(not(unix))]
compile_error!("faf only supports Unix");

mod cli;
mod config;
mod matcher;
mod util;
mod walker;
mod worker;

use clap::Parser;
use std::fs::File;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cli::{Cli, ColorMode};
use config::{EntryType, MatchMode, WalkConfig};
use matcher::MatchTarget;
use walker::walk_parallel;
use worker::Totals;

pub fn run() {
    let cli = Cli::parse();
    let prog = "faf";

    if cli.substr && cli.precise {
        eprintln!("error: cannot use -s and -p together");
        std::process::exit(2);
    }

    let mode = if cli.substr {
        MatchMode::Substr
    } else if cli.precise {
        MatchMode::Precise
    } else {
        MatchMode::Standard
    };

    // -f / -d are shorthands for --type f / --type d.
    if cli.file && cli.dir {
        eprintln!("{prog}: cannot use -f and -d together");
        std::process::exit(2);
    }
    let short_type = if cli.file {
        Some(EntryType::File)
    } else if cli.dir {
        Some(EntryType::Dir)
    } else {
        None
    };
    let flag_type = match cli.entry_type.as_deref() {
        None => None,
        Some("f") => Some(EntryType::File),
        Some("d") => Some(EntryType::Dir),
        Some("a") => Some(EntryType::Any),
        Some(other) => {
            eprintln!("{prog}: unknown --type '{}' (use f, d, or a)", other);
            std::process::exit(2);
        }
    };
    let entry_type = match (short_type, flag_type) {
        (Some(s), Some(f)) if s != f => {
            let flag = if cli.file { "-f" } else { "-d" };
            eprintln!(
                "{prog}: {flag} conflicts with --type {}",
                cli.entry_type.as_deref().unwrap()
            );
            std::process::exit(2);
        }
        (Some(t), _) | (_, Some(t)) => t,
        (None, None) => EntryType::Any,
    };

    let root = cli.root.unwrap_or_else(|| PathBuf::from("/"));

    let color = !cli.null
        && match cli.color {
            ColorMode::Never => false,
            ColorMode::Always => true,
            ColorMode::Auto => std::io::stdout().is_terminal(),
        };

    let config = Arc::new(WalkConfig {
        target: MatchTarget::new(&cli.target, mode, cli.ignore_case),
        max_depth: cli.max_depth,
        exclude: cli
            .exclude
            .into_iter()
            .map(|s| s.into_bytes().into_boxed_slice())
            .collect(),
        entry_type,
        null_terminate: cli.null,
        gitignore: cli.gitignore,
        verbose: cli.verbose,
        color,
    });

    let cache_writer = last_cache_path().and_then(CacheWriter::open);

    let start = Instant::now();
    let totals = Totals::new();

    walk_parallel(
        &root,
        Arc::clone(&config),
        Arc::clone(&totals),
        cache_writer.clone(),
    );

    if let Some(writer) = cache_writer {
        writer.finish();
    }

    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();
    let (scanned, found) = totals.snapshot();

    let files_per_sec = if secs > 0.0 {
        scanned as f64 / secs
    } else {
        f64::INFINITY
    };

    if !cli.quiet {
        eprintln!(
            "{prog}: scanned {} files in {:.2}s ({:.0} files/sec), found {} matches",
            scanned, secs, files_per_sec, found
        );
    }

    std::process::exit(if found > 0 { 0 } else { 1 });
}

/// Mirrors delfaf's cache path resolution so `faf ...` followed by a bare
/// `delfaf` finds the same file.
fn last_cache_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAF_LAST") {
        return Some(PathBuf::from(p));
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join("faf").join("last"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/faf/last"))
}

/// Streams matches to a temp file next to the cache path, then atomically
/// renames it into place once the walk finishes. Workers write in bounded
/// batches instead of holding every match in memory for the whole walk.
/// A failed cache write never affects faf's own exit code.
pub(crate) struct CacheWriter {
    tmp_path: PathBuf,
    final_path: PathBuf,
    file: Mutex<File>,
}

impl CacheWriter {
    fn open(final_path: PathBuf) -> Option<Arc<Self>> {
        if let Some(parent) = final_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut tmp_name = final_path.file_name()?.to_os_string();
        tmp_name.push(format!(".tmp.{}", std::process::id()));
        let tmp_path = final_path.with_file_name(tmp_name);
        let file = File::create(&tmp_path).ok()?;
        Some(Arc::new(Self {
            tmp_path,
            final_path,
            file: Mutex::new(file),
        }))
    }

    pub(crate) fn write(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.file.lock().unwrap().write_all(bytes);
        }
    }

    fn finish(self: Arc<Self>) {
        let Ok(this) = Arc::try_unwrap(self) else {
            return;
        };
        drop(this.file);
        if std::fs::rename(&this.tmp_path, &this.final_path).is_err() {
            // Stray leftover directory at the cache path; clear it and retry once.
            let _ = std::fs::remove_dir_all(&this.final_path);
            let _ = std::fs::rename(&this.tmp_path, &this.final_path);
        }
    }
}

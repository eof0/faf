use std::path::Path;
use std::sync::Arc;

use ignore::{WalkBuilder, WalkState};

use crate::CacheWriter;
use crate::config::WalkConfig;
use crate::util::entry_file_name_bytes;
use crate::worker::{Totals, WorkerState, process_entry, process_root};

/// Work-stealing parallel walk. Workers stream matches to stdout as found.
pub fn walk_parallel(
    root: &Path,
    config: Arc<WalkConfig>,
    totals: Arc<Totals>,
    cache_writer: Option<Arc<CacheWriter>>,
) {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_ignore(config.gitignore)
        .git_global(config.gitignore)
        .git_exclude(config.gitignore)
        .max_depth(config.max_depth);

    builder.build_parallel().run(|| {
        let mut state = WorkerState::new(
            Arc::clone(&config),
            Arc::clone(&totals),
            cache_writer.clone(),
        );
        Box::new(move |entry| {
            let e = match entry {
                Ok(e) => e,
                Err(err) => {
                    if state.config.verbose {
                        eprintln!("[ERROR] {err}");
                    }
                    return WalkState::Continue;
                }
            };
            let path = e.path();
            // d_type from readdir, no stat.
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            let is_root = e.depth() == 0;

            // The root was named explicitly, so -x never applies to it.
            if is_root {
                return if process_root(path, is_dir, &mut state) {
                    WalkState::Continue
                } else {
                    WalkState::Quit
                };
            }

            // Every other entry name comes from `readdir` and is read with
            // one reverse byte scan, here only when a rule needs it.
            let exclude = &state.config.exclude;
            let name = if exclude.has_rules(is_dir) {
                let name = entry_file_name_bytes(path);
                if exclude.excludes(path, name, is_dir) {
                    if state.config.verbose {
                        eprintln!("[SKIP] {}", path.display());
                    }
                    return if is_dir {
                        WalkState::Skip
                    } else {
                        WalkState::Continue
                    };
                }
                Some(name)
            } else {
                None
            };
            if process_entry(path, name, is_dir, &mut state) {
                WalkState::Continue
            } else {
                WalkState::Quit
            }
        })
    });
}

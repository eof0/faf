use std::path::Path;
use std::sync::Arc;

use ignore::{WalkBuilder, WalkState};

use crate::CacheWriter;
use crate::config::WalkConfig;
use crate::util::entry_file_name_bytes;
use crate::worker::{Totals, WorkerState, process_entry};

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

            // The root was named explicitly, so --exclude never applies to it.
            if is_dir && !is_root && excluded(path, &state.config.exclude) {
                if state.config.verbose {
                    eprintln!("[SKIP] {}", path.display());
                }
                return WalkState::Skip;
            }
            if process_entry(path, is_dir, is_root, &mut state) {
                WalkState::Continue
            } else {
                WalkState::Quit
            }
        })
    });
}

#[inline(always)]
fn excluded(path: &Path, exclude: &[Box<[u8]>]) -> bool {
    let name = entry_file_name_bytes(path);
    exclude.iter().any(|e| &**e == name)
}

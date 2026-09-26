use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use memchr::memmem::Finder;

use crate::config::{EntryType, MatchMode};
use crate::matcher::MatchTarget;

/// How the name patterns of one `-x` group are matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMode {
    /// Exact full filename. The default and `-xp`.
    Precise,
    /// Substring of the filename, or of the path below the root if the pattern has a slash. `-xs`.
    Substr,
}

/// One `-x`, `-xf`, `-xd`, `-xp`, `-xs` or stacked group as parsed from argv.
pub struct ExcludeSpec {
    pub patterns: Vec<String>,
    pub entry_type: EntryType,
    pub mode: NameMode,
}

/// Substring of the emitted path below the root.
#[derive(Clone)]
struct PathSubstr {
    finder: Finder<'static>,
    /// True when every ancestor directory below the root was already
    /// checked against this rule, so only the part of the path that
    /// overlaps the final component can hold a new hit.
    narrow: bool,
}

/// Rules for one entry type, grouped by kind so the hot path runs only
/// the kinds that were given.
#[derive(Default, Clone)]
struct Rules {
    names_precise: Vec<MatchTarget>,
    names_substr: Vec<MatchTarget>,
    /// `/rel` bytes, compared with the path below the root.
    paths: Vec<Box<[u8]>>,
    path_substrs: Vec<PathSubstr>,
    /// Any vector non-empty. One load on the hot path.
    any: bool,
}

impl Rules {
    fn push_name(&mut self, target: MatchTarget, mode: NameMode) {
        let vec = match mode {
            NameMode::Precise => &mut self.names_precise,
            NameMode::Substr => &mut self.names_substr,
        };
        if !vec
            .iter()
            .any(|t| t.needle() == target.needle() && t.ignore_case == target.ignore_case)
        {
            vec.push(target);
        }
        self.any = true;
    }

    fn push_path(&mut self, bytes: Box<[u8]>) {
        if !self.paths.contains(&bytes) {
            self.paths.push(bytes);
        }
        self.any = true;
    }

    fn push_path_substr(&mut self, needle: &[u8], narrow: bool) {
        if !self
            .path_substrs
            .iter()
            .any(|r| r.finder.needle() == needle && r.narrow == narrow)
        {
            self.path_substrs.push(PathSubstr {
                finder: Finder::new(needle).into_owned(),
                narrow,
            });
        }
        self.any = true;
    }

    /// `full` is the emitted path, `name` its final component, and
    /// `below_root` the offset of the `/` after the root.
    #[inline(always)]
    fn matches(&self, full: &[u8], name: &[u8], below_root: usize) -> bool {
        if self.names_precise.iter().any(|t| t.equals(name)) {
            return true;
        }
        if self.names_substr.iter().any(|t| t.contains(name)) {
            return true;
        }
        if self.paths.is_empty() && self.path_substrs.is_empty() {
            return false;
        }
        let below = &full[below_root..];
        if self.paths.iter().any(|p| &**p == below) {
            return true;
        }
        // The walker visits a directory before anything inside it, so a
        // narrow rule was already run over the parent's `below` slice and
        // found nothing. A hit here must overlap the trailing `/name`.
        let parent_len = below.len().saturating_sub(name.len() + 1);
        self.path_substrs.iter().any(|r| {
            let start = if r.narrow {
                parent_len.saturating_sub(r.finder.needle().len() - 1)
            } else {
                0
            };
            r.finder.find(&below[start..]).is_some()
        })
    }
}

/// Compiled exclude rules, split by the entry type they apply to so the
/// common case of "no rule for this type" is one boolean load.
#[derive(Default)]
pub struct ExcludeSet {
    files: Rules,
    dirs: Rules,
    /// Offset of the `/` that separates the root, as the walker spells it,
    /// from the rest of every emitted path.
    below_root: usize,
    /// One line per path pattern saying what it resolved to. Only filled
    /// when built with `verbose`.
    pub notes: Vec<String>,
}

/// What a path pattern turned into.
enum PathRule {
    /// Path relative to the root, one exact entry.
    Exact(PathBuf),
    /// Needle for the path below the root.
    Substr(String),
}

impl ExcludeSet {
    /// `root` is the walk root exactly as the user spelled it, since that
    /// is the prefix the walker puts on every emitted path. `home` backs
    /// `~` expansion and is None when HOME is unset.
    pub fn build(
        specs: Vec<ExcludeSpec>,
        root: &Path,
        home: Option<&OsStr>,
        ignore_case: bool,
        verbose: bool,
    ) -> Self {
        let mut set = Self {
            // `root.join("")` is the root with exactly one trailing slash.
            below_root: root.join("").as_os_str().len() - 1,
            ..Self::default()
        };
        if specs.iter().all(|s| s.patterns.is_empty()) {
            return set;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let root_abs = lexical_absolute(root, &cwd);
        for spec in specs {
            let match_mode = match spec.mode {
                NameMode::Precise => MatchMode::Precise,
                NameMode::Substr => MatchMode::Substr,
            };
            for raw in spec.patterns {
                let pattern = expand_tilde(&raw, home);
                if pattern.is_empty() {
                    continue;
                }
                let is_path = pattern.contains('/') || pattern == "." || pattern == "..";
                if !is_path {
                    let target = MatchTarget::new(&pattern, match_mode, ignore_case);
                    set.each(spec.entry_type, |rules, _| {
                        rules.push_name(target.clone(), spec.mode)
                    });
                    continue;
                }
                match resolve_path(&pattern, spec.mode, &root_abs, &cwd) {
                    Ok(PathRule::Exact(rel)) => {
                        let needle = format!("/{}", rel.display());
                        if verbose {
                            let spelled = root.join(&rel);
                            let missing = if std::fs::symlink_metadata(&spelled).is_ok() {
                                ""
                            } else {
                                " (does not exist)"
                            };
                            set.notes
                                .push(format!("[EXCLUDE] {raw} -> {}{missing}", spelled.display()));
                        }
                        let bytes = needle.into_bytes().into_boxed_slice();
                        set.each(spec.entry_type, |rules, _| rules.push_path(bytes.clone()));
                    }
                    Ok(PathRule::Substr(needle)) => {
                        if verbose {
                            set.notes.push(format!(
                                "[EXCLUDE] {raw} -> substring {needle} of the path below the root"
                            ));
                        }
                        set.each(spec.entry_type, |rules, narrow| {
                            rules.push_path_substr(needle.as_bytes(), narrow)
                        });
                    }
                    Err(why) => {
                        if verbose {
                            set.notes.push(format!("[EXCLUDE] {raw} {why}, ignored"));
                        }
                    }
                }
            }
        }
        set
    }

    /// Runs `f` on each rule set the entry type covers. The flag says
    /// whether the rule also lands in `dirs`, which is what makes the
    /// narrow substring haystack valid for it.
    fn each(&mut self, entry_type: EntryType, mut f: impl FnMut(&mut Rules, bool)) {
        match entry_type {
            EntryType::File => f(&mut self.files, false),
            EntryType::Dir => f(&mut self.dirs, true),
            EntryType::Any => {
                f(&mut self.dirs, true);
                f(&mut self.files, true);
            }
        }
    }

    /// Whether any rule applies to this entry type.
    #[inline(always)]
    pub fn has_rules(&self, is_dir: bool) -> bool {
        if is_dir {
            self.dirs.any
        } else {
            self.files.any
        }
    }

    /// `path` is a non-root walk entry and `name` its final component.
    #[inline(always)]
    pub fn excludes(&self, path: &Path, name: &[u8], is_dir: bool) -> bool {
        let rules = if is_dir { &self.dirs } else { &self.files };
        rules.matches(path.as_os_str().as_bytes(), name, self.below_root)
    }
}

/// `~` and `~/x` expand through HOME. A quoted comma list never reaches
/// the shell's own tilde expansion, so faf has to do it.
fn expand_tilde(pattern: &str, home: Option<&OsStr>) -> String {
    let rest = match pattern {
        "~" => Some(""),
        p => p.strip_prefix("~/"),
    };
    match (rest, home) {
        (Some(rest), Some(home)) => {
            let mut out = PathBuf::from(home);
            if !rest.is_empty() {
                out.push(rest);
            }
            out.to_string_lossy().into_owned()
        }
        _ => pattern.to_owned(),
    }
}

/// Turns a pattern with a slash into a rule, or says why it cannot match.
///
/// Everything is lexical. The walker never follows symlinks, so neither
/// side may be canonicalized.
///
/// Exact: a relative pattern is taken from the current directory first,
/// like every other path on the command line and like shell tab
/// completion. If that lands outside the root it is retried relative to
/// the root, so `faf main ~ -x projects/x` still means `~/projects/x`.
///
/// Substring: an absolute pattern below the root is rebased so it has the
/// same spelling as the haystack, keeping its trailing slash. Anything
/// else, including a fragment like `/c/`, is used verbatim.
fn resolve_path(
    pattern: &str,
    mode: NameMode,
    root_abs: &Path,
    cwd: &Path,
) -> Result<PathRule, &'static str> {
    const ROOT: &str = "is the root";
    match mode {
        NameMode::Precise => {
            let candidate = Path::new(pattern.trim_end_matches('/'));
            // `join` with an absolute right side yields it unchanged, so
            // this covers absolute patterns too.
            let rel = rel_under(candidate, root_abs, cwd)
                .or_else(|| rel_under(&root_abs.join(candidate), root_abs, cwd))
                .ok_or("is outside the root")?;
            if rel.as_os_str().is_empty() {
                return Err(ROOT);
            }
            Ok(PathRule::Exact(rel))
        }
        NameMode::Substr => {
            let candidate = Path::new(pattern);
            let rel = match candidate.is_absolute() {
                true => rel_under(candidate, root_abs, cwd),
                false => None,
            };
            let Some(rel) = rel else {
                return Ok(PathRule::Substr(pattern.to_owned()));
            };
            if rel.as_os_str().is_empty() {
                return Err(ROOT);
            }
            let mut needle = format!("/{}", rel.display());
            if pattern.ends_with('/') {
                needle.push('/');
            }
            Ok(PathRule::Substr(needle))
        }
    }
}

/// `candidate` made absolute, as a path relative to `root_abs`.
fn rel_under(candidate: &Path, root_abs: &Path, cwd: &Path) -> Option<PathBuf> {
    lexical_absolute(candidate, cwd)
        .strip_prefix(root_abs)
        .ok()
        .map(Path::to_path_buf)
}

/// Absolute path with `.` and `..` folded away without touching the disk.
fn lexical_absolute(path: &Path, cwd: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in cwd.join(path).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::entry_file_name_bytes;

    fn spec(patterns: &[&str], entry_type: EntryType, mode: NameMode) -> ExcludeSpec {
        ExcludeSpec {
            patterns: patterns.iter().map(|s| (*s).to_owned()).collect(),
            entry_type,
            mode,
        }
    }

    fn build(patterns: &[&str], entry_type: EntryType, mode: NameMode, root: &str) -> ExcludeSet {
        ExcludeSet::build(
            vec![spec(patterns, entry_type, mode)],
            Path::new(root),
            None,
            false,
            true,
        )
    }

    fn excluded(set: &ExcludeSet, path: &str, is_dir: bool) -> bool {
        let p = Path::new(path);
        set.excludes(p, entry_file_name_bytes(p), is_dir)
    }

    fn resolve(pattern: &str, mode: NameMode, root: &str) -> Result<String, &'static str> {
        resolve_path(pattern, mode, Path::new(root), Path::new("/cwd")).map(|r| match r {
            PathRule::Exact(rel) => format!("exact {}", rel.display()),
            PathRule::Substr(needle) => format!("substr {needle}"),
        })
    }

    #[test]
    fn lexical_absolute_folds_dot_and_dotdot() {
        let cwd = Path::new("/cwd");
        assert_eq!(
            lexical_absolute(Path::new("/a/./b/../c/"), cwd),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            lexical_absolute(Path::new("x/../y"), cwd),
            PathBuf::from("/cwd/y")
        );
    }

    #[test]
    fn below_root_offset_lands_on_the_separator_for_every_root_spelling() {
        for (root, entry) in [
            (".", "./x"),
            ("/", "/x"),
            ("home", "home/x"),
            ("home/", "home/x"),
            ("/a/b", "/a/b/x"),
        ] {
            let set = build(&[], EntryType::Any, NameMode::Precise, root);
            assert_eq!(&entry.as_bytes()[set.below_root..], b"/x", "root {root}");
        }
    }

    #[test]
    fn no_patterns_means_no_rules_for_either_type() {
        let set = build(&[], EntryType::Any, NameMode::Precise, ".");
        assert!(!set.has_rules(true) && !set.has_rules(false));
        let set = build(&["x"], EntryType::Dir, NameMode::Precise, ".");
        assert!(set.has_rules(true) && !set.has_rules(false));
    }

    #[test]
    fn relative_path_pattern_is_spelled_with_root_prefix() {
        let set = build(&["src/gen/"], EntryType::Any, NameMode::Precise, ".");
        assert!(excluded(&set, "./src/gen", true));
        assert!(excluded(&set, "./src/gen", false));
        assert!(!excluded(&set, "./src/gen/x", true));
        assert!(!excluded(&set, "./src", true));
    }

    #[test]
    fn relative_pattern_prefers_cwd_then_falls_back_to_root() {
        // Root is a subdirectory of cwd: the cwd-relative spelling wins.
        assert_eq!(
            resolve("src/gen", NameMode::Precise, "/cwd/src"),
            Ok("exact gen".into())
        );
        // Not under the root from cwd, so it is taken relative to the root.
        assert_eq!(
            resolve("gen", NameMode::Precise, "/cwd/src"),
            Ok("exact gen".into())
        );
        assert_eq!(
            resolve("/cwd/src/gen/", NameMode::Precise, "/cwd/src"),
            Ok("exact gen".into())
        );
    }

    #[test]
    fn absolute_path_pattern_is_rebased_onto_root_spelling() {
        let cwd = std::env::current_dir().unwrap();
        let pattern = cwd.join("src/gen").to_string_lossy().into_owned();
        let set = build(&[&pattern], EntryType::Any, NameMode::Precise, ".");
        assert!(excluded(&set, "./src/gen", true));
    }

    #[test]
    fn path_outside_root_or_equal_to_root_produces_no_rule() {
        let set = build(
            &["/nowhere/else", ".", "./"],
            EntryType::Any,
            NameMode::Precise,
            ".",
        );
        assert!(!excluded(&set, "/nowhere/else", true));
        assert!(!set.has_rules(true) && !set.has_rules(false));
        assert_eq!(
            set.notes,
            [
                "[EXCLUDE] /nowhere/else is outside the root, ignored",
                "[EXCLUDE] . is the root, ignored",
                "[EXCLUDE] ./ is the root, ignored",
            ]
        );
    }

    #[test]
    fn notes_are_only_built_when_verbose() {
        let set = ExcludeSet::build(
            vec![spec(
                &["/nowhere", "a/b"],
                EntryType::Any,
                NameMode::Precise,
            )],
            Path::new("."),
            None,
            false,
            false,
        );
        assert!(set.notes.is_empty());
        assert!(excluded(&set, "./a/b", true));
    }

    #[test]
    fn name_rules_respect_entry_type() {
        let set = build(&["build"], EntryType::Dir, NameMode::Precise, "/r");
        assert!(excluded(&set, "/r/a/build", true));
        assert!(!excluded(&set, "/r/a/build", false));
        assert!(!excluded(&set, "/r/a/build.rs", true));
    }

    #[test]
    fn duplicate_rules_are_stored_once() {
        let set = ExcludeSet::build(
            vec![
                spec(
                    &["a", "a", "/r/p", "/r/p", "x/"],
                    EntryType::Any,
                    NameMode::Precise,
                ),
                spec(&["a", "/r/p", "x/"], EntryType::Dir, NameMode::Precise),
                spec(&["x/"], EntryType::Any, NameMode::Substr),
            ],
            Path::new("/r"),
            None,
            false,
            false,
        );
        assert_eq!(set.dirs.names_precise.len(), 1);
        assert_eq!(set.dirs.paths.len(), 2);
        assert_eq!(set.dirs.path_substrs.len(), 1);
        assert_eq!(set.files.names_precise.len(), 1);
    }

    #[test]
    fn substr_on_name_or_below_root_when_slashed() {
        let set = build(&["head", "/c/"], EntryType::Any, NameMode::Substr, "/c/r");
        assert!(excluded(&set, "/c/r/mainheader.tsx", false));
        assert!(excluded(&set, "/c/r/c/main.c", false));
        // The root's own /c/ is above the haystack.
        assert!(!excluded(&set, "/c/r/rust/main.rs", false));
        // Trailing slash is kept, so a prefix of a name does not match.
        assert!(!excluded(&set, "/c/r/cache/main.c", false));
    }

    #[test]
    fn narrow_substr_still_sees_hits_that_straddle_the_final_component() {
        // A hit spanning the parent's last byte and the new `/name` must
        // survive the narrowed haystack.
        let set = build(&["b/c"], EntryType::Any, NameMode::Substr, "/r");
        assert!(set.dirs.path_substrs[0].narrow);
        assert!(excluded(&set, "/r/a/b/c", true));
        assert!(excluded(&set, "/r/a/b/c", false));
        assert!(!excluded(&set, "/r/a/b/d", true));
        // A depth-one entry has no checked parent: full haystack.
        let set = build(&["/x"], EntryType::Any, NameMode::Substr, "/r");
        assert!(excluded(&set, "/r/x", false));
        // Files-only rules never saw the directories, so they stay wide.
        let set = build(&["/c/"], EntryType::File, NameMode::Substr, "/r");
        assert!(!set.files.path_substrs[0].narrow);
        assert!(excluded(&set, "/r/c/main.c", false));
    }

    #[test]
    fn absolute_substr_pattern_is_rebased_below_root() {
        assert_eq!(
            resolve("/r/c/", NameMode::Substr, "/r"),
            Ok("substr /c/".into())
        );
        assert_eq!(
            resolve("/r/c", NameMode::Substr, "/r"),
            Ok("substr /c".into())
        );
        assert_eq!(resolve("/r/", NameMode::Substr, "/r"), Err("is the root"));
        // Not below the root: a literal fragment, not a path.
        assert_eq!(
            resolve("/x/c", NameMode::Substr, "/r"),
            Ok("substr /x/c".into())
        );
        assert_eq!(
            resolve("c/", NameMode::Substr, "/r"),
            Ok("substr c/".into())
        );
    }

    #[test]
    fn tilde_expands_through_home() {
        let home = OsStr::new("/home/tester");
        assert_eq!(expand_tilde("~", Some(home)), "/home/tester");
        assert_eq!(expand_tilde("~/x/", Some(home)), "/home/tester/x/");
        assert_eq!(expand_tilde("~x", Some(home)), "~x");
        assert_eq!(expand_tilde("a/~/b", Some(home)), "a/~/b");
        assert_eq!(expand_tilde("~/x", None), "~/x");
    }
}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A throwaway tree mirroring the shape from the feature request:
///
/// home/
///   projects/work/programming/typescript/source-code/main.ts
///   projects/work/programming/c/main.c
///   projects/work/programming/rust/main.rs
///   projects/work/programming/rust/main.lua
///   projects/work/programming/rust/mainheader.tsx
///   projects/work/programming/rust/domain.rs
///   projects/main/               (a directory named main)
struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("faf-exclude-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let prog = home.join("projects/work/programming");
        for dir in ["typescript/source-code", "c", "rust"] {
            fs::create_dir_all(prog.join(dir)).unwrap();
        }
        fs::create_dir_all(home.join("projects/main")).unwrap();
        for file in [
            "typescript/source-code/main.ts",
            "c/main.c",
            "rust/main.rs",
            "rust/main.lua",
            "rust/mainheader.tsx",
            "rust/domain.rs",
        ] {
            fs::write(prog.join(file), b"").unwrap();
        }
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// Runs faf with `HOME` pointed at the tree so `~` expands into it.
    /// The cache file is also redirected so tests never touch the real one.
    fn run(&self, cwd: &Path, args: &[&str]) -> (i32, Vec<String>, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_faf"))
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.home())
            .env("FAF_LAST", self.root.join("cache"))
            .output()
            .expect("run faf");
        let mut lines: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        (
            out.status.code().unwrap(),
            lines,
            String::from_utf8(out.stderr).unwrap(),
        )
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn rel(lines: &[String], base: &Path) -> Vec<String> {
    let base = base.to_str().unwrap();
    lines
        .iter()
        .map(|l| {
            l.strip_prefix(base)
                .unwrap_or(l)
                .trim_start_matches('/')
                .to_owned()
        })
        .collect()
}

#[test]
fn baseline_without_exclude_finds_everything_named_main() {
    let t = Tree::new();
    let home = t.home();
    let (code, lines, _) = t.run(&t.root, &["main", home.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(
        rel(&lines, &home),
        [
            "projects/main",
            "projects/work/programming/c/main.c",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/rust/main.rs",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn feature_request_example_excludes_dirs_and_files_after_root() {
    let t = Tree::new();
    let home = t.home();
    // faf -f main ~ -x "~/projects/work/programming/typescript,~/projects/work/programming/c/,main.lua,main.cpp"
    let (code, lines, _) = t.run(
        &t.root,
        &[
            "-f",
            "main",
            home.to_str().unwrap(),
            "-x",
            "~/projects/work/programming/typescript,~/projects/work/programming/c/,main.lua,main.cpp",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(
        rel(&lines, &home),
        ["projects/work/programming/rust/main.rs"]
    );
}

#[test]
fn absolute_path_exclude_with_dot_root() {
    let t = Tree::new();
    let home = t.home();
    let ts = home.join("projects/work/programming/typescript");
    let (code, lines, _) = t.run(&home, &["main", ".", "-x", ts.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(
        lines,
        [
            "./projects/main",
            "./projects/work/programming/c/main.c",
            "./projects/work/programming/rust/main.lua",
            "./projects/work/programming/rust/main.rs",
        ]
    );
}

#[test]
fn relative_path_exclude_resolves_against_root() {
    let t = Tree::new();
    let (_, lines, _) = t.run(
        &t.root,
        &["main", "home", "-x", "projects/work/programming/rust/"],
    );
    assert_eq!(
        lines,
        [
            "home/projects/main",
            "home/projects/work/programming/c/main.c",
            "home/projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn root_with_trailing_slash_still_matches_path_excludes() {
    let t = Tree::new();
    let (_, lines, _) = t.run(
        &t.root,
        &[
            "main",
            "home/",
            "-x",
            "home/projects/work/programming/rust/",
        ],
    );
    assert_eq!(
        lines,
        [
            "home/projects/main",
            "home/projects/work/programming/c/main.c",
            "home/projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn bare_name_excludes_files_and_directories_exactly() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &t.root,
        &["main", home.to_str().unwrap(), "-x", "main,main.rs"],
    );
    assert_eq!(
        rel(&lines, &home),
        [
            "projects/work/programming/c/main.c",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn xd_only_excludes_directories() {
    let t = Tree::new();
    let home = t.home();
    // A directory named main is excluded, the file main.rs stays (it is not a dir).
    let (_, lines, _) = t.run(
        &t.root,
        &["main", home.to_str().unwrap(), "-xd", "main,main.rs"],
    );
    assert_eq!(
        rel(&lines, &home),
        [
            "projects/work/programming/c/main.c",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/rust/main.rs",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn xf_only_excludes_files() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &t.root,
        &["main", home.to_str().unwrap(), "-xf", "main,main.rs"],
    );
    assert_eq!(
        rel(&lines, &home),
        [
            "projects/main",
            "projects/work/programming/c/main.c",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn xs_excludes_by_name_substring_and_path_substring() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &t.root,
        &["-s", "main", home.to_str().unwrap(), "-xs", "header,/c/"],
    );
    assert_eq!(
        rel(&lines, &home),
        [
            "projects/main",
            "projects/work/programming/rust/domain.rs",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/rust/main.rs",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn xp_is_explicit_exact_name_and_stacks_with_type() {
    let t = Tree::new();
    let home = t.home();
    let (_, a, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xp", "main.rs"]);
    let (_, b, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-x", "main.rs"]);
    assert_eq!(a, b);
    let (_, c, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xfs", ".r"]);
    assert_eq!(
        rel(&c, &home),
        [
            "projects/main",
            "projects/work/programming/c/main.c",
            "projects/work/programming/rust/main.lua",
            "projects/work/programming/typescript/source-code/main.ts",
        ]
    );
}

#[test]
fn long_forms_and_repeats_work() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &t.root,
        &[
            "main",
            home.to_str().unwrap(),
            "--exclude-dir",
            "typescript",
            "--exclude",
            "main.lua",
            "-x",
            "main.c",
        ],
    );
    assert_eq!(
        rel(&lines, &home),
        ["projects/main", "projects/work/programming/rust/main.rs"]
    );
}

#[test]
fn x_with_attached_non_modifier_value_is_a_name() {
    let t = Tree::new();
    let home = t.home();
    // -xmain.rs is clap's attached-value form, not a modifier set.
    let (_, lines, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xmain.rs"]);
    assert!(!lines.iter().any(|l| l.ends_with("main.rs")));
    assert!(lines.iter().any(|l| l.ends_with("main.c")));
}

#[test]
fn conflicting_modifiers_are_usage_errors() {
    let t = Tree::new();
    let home = t.home();
    for bad in ["-xfd", "-xps", "-xff"] {
        let (code, _, err) = t.run(&t.root, &["main", home.to_str().unwrap(), bad, "x"]);
        assert_eq!(code, 2, "{bad} should be a usage error, stderr: {err}");
        assert!(err.contains(bad), "stderr should name the flag: {err}");
    }
}

#[test]
fn ignore_case_applies_to_name_excludes() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &t.root,
        &["-i", "MAIN", home.to_str().unwrap(), "-x", "MAIN.RS"],
    );
    assert!(!lines.iter().any(|l| l.ends_with("main.rs")));
    assert!(lines.iter().any(|l| l.ends_with("main.c")));
}

#[test]
fn root_itself_is_never_excluded() {
    let t = Tree::new();
    let rust = t.home().join("projects/work/programming/rust");
    let (code, lines, _) = t.run(&t.root, &["main", rust.to_str().unwrap(), "-x", "rust"]);
    assert_eq!(code, 0);
    assert_eq!(lines.len(), 2);
}

#[test]
fn verbose_reports_how_each_path_exclude_resolved() {
    let t = Tree::new();
    let home = t.home();
    let (_, _, err) = t.run(
        &home,
        &[
            "-v",
            "main",
            ".",
            "-x",
            "~/projects/work/programming/c/,projects/nope,/elsewhere/x,./,main.lua",
        ],
    );
    let lines: Vec<&str> = err.lines().filter(|l| l.starts_with("[EXCLUDE]")).collect();
    assert_eq!(
        lines,
        [
            "[EXCLUDE] ~/projects/work/programming/c/ -> ./projects/work/programming/c".to_owned(),
            "[EXCLUDE] projects/nope -> ./projects/nope (does not exist)".to_owned(),
            "[EXCLUDE] /elsewhere/x is outside the root, ignored".to_owned(),
            "[EXCLUDE] ./ is the root, ignored".to_owned(),
        ]
    );
}

#[test]
fn xs_tilde_path_is_rebased_below_root() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(
        &home,
        &["-s", "main", ".", "-xs", "~/projects/work/programming/c/"],
    );
    assert!(!lines.iter().any(|l| l.ends_with("main.c")), "{lines:?}");
    assert!(lines.iter().any(|l| l.ends_with("main.rs")));
}

#[test]
fn xs_path_substring_ignores_the_root_prefix() {
    let t = Tree::new();
    let c = t.home().join("projects/work/programming/c");
    // The root's own path contains /c/, which must not prune everything.
    let (code, lines, _) = t.run(&t.root, &["main", c.to_str().unwrap(), "-xs", "/c/"]);
    assert_eq!(code, 0);
    assert_eq!(lines.len(), 1);
}

#[test]
fn xs_keeps_trailing_slash_so_prefixes_do_not_over_match() {
    let t = Tree::new();
    let home = t.home();
    let (_, lines, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xs", "rus/"]);
    assert!(
        lines.iter().any(|l| l.ends_with("rust/main.rs")),
        "{lines:?}"
    );
}

#[test]
fn path_excludes_do_not_follow_symlinks() {
    use std::os::unix::fs::symlink;
    let t = Tree::new();
    let home = t.home();
    symlink(&home, t.root.join("rl")).unwrap();
    symlink("work/programming/c", home.join("projects/clink")).unwrap();

    // Root spelled through a symlink: the pattern spelled the same way resolves under it.
    let (_, lines, _) = t.run(&t.root, &["main", "rl", "-x", "rl/projects/main"]);
    assert!(
        !lines.iter().any(|l| l.ends_with("projects/main")),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l.ends_with("main.c")));

    // Excluding a symlink entry must not prune its target directory.
    let (_, lines, _) = t.run(&t.root, &["main", "home", "-x", "home/projects/clink"]);
    assert!(lines.iter().any(|l| l.ends_with("c/main.c")), "{lines:?}");
}

#[test]
fn x_modifiers_work_inside_a_short_flag_cluster_and_with_equals() {
    let t = Tree::new();
    let home = t.home();
    // -sxd is -s plus --exclude-dir, so the next argument is the exclude value.
    let (_, lines, _) = t.run(&t.root, &["-sxd", "main", "main", home.to_str().unwrap()]);
    assert!(
        !lines.iter().any(|l| l.ends_with("projects/main")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.ends_with("mainheader.tsx")),
        "{lines:?}"
    );

    let (_, lines, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xf=main.rs"]);
    assert!(!lines.iter().any(|l| l.ends_with("main.rs")), "{lines:?}");
    assert!(lines.iter().any(|l| l.ends_with("main.c")));
}

#[test]
fn xs_needle_inside_an_ancestor_prunes_everything_below_it() {
    // The narrowed haystack for files relies on the walker visiting a
    // directory before anything inside it: /work/p lies wholly inside
    // projects/work/programming, so that directory must be pruned and no
    // file under it may leak through a file-level check.
    let t = Tree::new();
    let home = t.home();
    let (code, lines, _) = t.run(&t.root, &["main", home.to_str().unwrap(), "-xs", "/work/p"]);
    assert_eq!(code, 0);
    assert_eq!(rel(&lines, &home), ["projects/main"]);
}

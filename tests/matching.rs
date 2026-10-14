use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf, PathBuf);

impl Tree {
    fn new(files: &[&str]) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("faf-matching-{}-{id}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let tree = Self(root.join("search"), root);
        fs::create_dir(&tree.0).unwrap();
        for file in files {
            tree.file(file.as_bytes());
        }
        tree
    }

    fn file(&self, name: &[u8]) {
        let path = self.0.join(std::ffi::OsString::from_vec(name.to_vec()));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_faf"))
            .args(args)
            .arg(&self.0)
            .args(["-q", "--color", "never"])
            .env("FAF_LAST", self.1.join("cache/last"))
            .output()
            .unwrap()
    }

    fn matches(&self, args: &[&str], expected: &[&str]) {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(if expected.is_empty() { 1 } else { 0 }),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let prefix = format!("{}/", self.0.display());
        let mut actual: Vec<_> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| line.strip_prefix(&prefix).unwrap().to_owned())
            .collect();
        actual.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(actual, expected, "{args:?}");
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.1).unwrap();
    }
}

#[test]
fn extension_mode_matches_only_the_requested_suffix_recursively() {
    let tree = Tree::new(&["app.js", "app.json", "app.js.map", "nested/other.js", ".js"]);
    for args in [["-e", "js"], ["--extension", ".js"]] {
        tree.matches(&args, &["app.js", "nested/other.js", ".js"]);
    }
}

#[test]
fn extension_and_suffix_glob_find_the_same_entries() {
    let tree = Tree::new(&[
        "notes.txt",
        "nested/report.txt",
        "noext",
        "notxt",
        "notes.txt.bak",
        ".txt",
    ]);
    for args in [vec!["-e", "txt"], vec!["*.txt"]] {
        tree.matches(&args, &["notes.txt", "nested/report.txt", ".txt"]);
    }
}

#[test]
fn compound_extension_is_a_suffix_match() {
    let tree = Tree::new(&["backup.tar.gz", "archive.gz", "backup.tar.gz.bak"]);
    tree.matches(&["-e", "tar.gz"], &["backup.tar.gz"]);
    tree.matches(&["*.tar.gz"], &["backup.tar.gz"]);
}

#[test]
fn prefix_glob_matches_arbitrary_or_empty_suffixes() {
    let tree = Tree::new(&[
        "File_Name_Index_",
        "File_Name_Index_abc",
        "File_Name_Index_42.txt",
        "File_Name_Index_ü",
        "preFile_Name_Index_42",
    ]);
    tree.matches(
        &["File_Name_Index_*"],
        &[
            "File_Name_Index_",
            "File_Name_Index_abc",
            "File_Name_Index_42.txt",
            "File_Name_Index_ü",
        ],
    );
}

#[test]
fn globs_match_the_full_filename_without_stripping_extensions() {
    let tree = Tree::new(&["main.rs", "main.go", "my-main.rs", "main.rs.bak", "mainrs"]);
    tree.matches(&["main*.rs"], &["main.rs"]);
    tree.matches(&["*main.rs"], &["main.rs", "my-main.rs"]);
}

#[test]
fn contains_glob_matches_the_same_names_as_substring() {
    let tree = Tree::new(&["foo", "wordsfoowords.txt", "herefoohere", "bar"]);
    for args in [vec!["*foo*"], vec!["-s", "foo"]] {
        tree.matches(&args, &["foo", "wordsfoowords.txt", "herefoohere"]);
    }
}

#[test]
fn multiple_stars_require_ordered_nonoverlapping_segments() {
    let tree = Tree::new(&[
        "abc",
        "a-one-b-two-c",
        "a-bc",
        "acb",
        "abc.bak",
        "zabc",
        "abcbc",
    ]);
    tree.matches(&["a*b*c"], &["abc", "a-one-b-two-c", "a-bc", "abcbc"]);
    tree.matches(&["ab*bc"], &["abcbc"]);
    tree.matches(&["a*b*b*c"], &["abcbc"]);
}

#[test]
fn repeated_stars_are_equivalent_to_one_star() {
    let tree = Tree::new(&["a.txt", ".hidden.txt", "nested/b.txt", "a.rs"]);
    tree.matches(&["***.txt"], &["a.txt", ".hidden.txt", "nested/b.txt"]);
    tree.matches(&["a**.txt"], &["a.txt"]);
}

#[test]
fn star_matches_all_types_including_hidden_entries() {
    let tree = Tree::new(&["a.txt", ".hidden", "nested/b.js"]);
    tree.matches(&["-f", "*"], &["a.txt", ".hidden", "nested/b.js"]);
}

#[test]
fn extension_and_glob_respect_type_filters_and_excludes() {
    let tree = Tree::new(&["a.txt", "nested/b.txt", "skip/c.txt"]);
    fs::create_dir(tree.0.join("dir.txt")).unwrap();
    for args in [vec!["-e", "txt"], vec!["*.txt"]] {
        tree.matches(&args, &["a.txt", "nested/b.txt", "skip/c.txt", "dir.txt"]);
        let mut files = args.clone();
        files.extend(["-f", "-xd", "skip"]);
        tree.matches(&files, &["a.txt", "nested/b.txt"]);
        let mut dirs = args;
        dirs.push("-d");
        tree.matches(&dirs, &["dir.txt"]);
    }
}

#[test]
fn escaped_asterisks_match_literal_filenames() {
    let tree = Tree::new(&[
        "report*.txt",
        "report*.rs",
        "report-one.txt",
        "a*b.txt",
        "a*bc.txt",
    ]);
    tree.matches(&[r"report\*.txt"], &["report*.txt"]);
    tree.matches(&[r"a\*b*.txt"], &["a*b.txt", "a*bc.txt"]);
    tree.matches(&["-s", r"a\*b"], &["a*b.txt", "a*bc.txt"]);
    tree.matches(&["-p", r"report\*.rs"], &["report*.rs"]);
}

#[test]
fn escaped_backslashes_can_be_combined_with_wildcards() {
    let tree = Tree::new(&[r"report\one.txt", "report-one.txt", r"report\*.txt"]);
    tree.matches(&[r"report\\*.txt"], &[r"report\one.txt", r"report\*.txt"]);
    tree.matches(&[r"report\\\*.txt"], &[r"report\*.txt"]);
}

#[test]
fn wildcard_syntax_conflicts_with_explicit_matching_modes() {
    let tree = Tree::new(&[]);
    for flag in ["-s", "-p", "-e"] {
        let output = tree.run(&[flag, "*.txt"]);
        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("wildcard"), "{stderr}");
        assert!(stderr.contains(flag), "{stderr}");
    }
    assert!(
        !tree.1.join("cache").exists(),
        "invalid patterns must not start a search"
    );
}

#[test]
fn extension_conflicts_with_substring_and_precise_modes() {
    let tree = Tree::new(&[]);
    for flag in ["-s", "-p"] {
        let output = tree.run(&["-e", flag, "txt"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn trailing_escape_and_empty_extensions_are_usage_errors() {
    let tree = Tree::new(&[]);
    for args in [
        vec!["name\\"],
        vec!["-e", ""],
        vec!["-e", "."],
        vec!["-e", "path/txt"],
    ] {
        let output = tree.run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn case_insensitive_extension_and_glob_preserve_unicode_support() {
    let tree = Tree::new(&["UPPER.TXT", "über.txt", "ÜBER.TXT", "bücher.Ü", "notxt"]);
    tree.matches(&["-ie", "TXT"], &["UPPER.TXT", "über.txt", "ÜBER.TXT"]);
    tree.matches(&["-i", "*ÜBER*.txt"], &["über.txt", "ÜBER.TXT"]);
    tree.matches(&["-ie", "ü"], &["bücher.Ü"]);
    tree.matches(&["*.TXT"], &["UPPER.TXT", "ÜBER.TXT"]);
}

#[test]
fn non_utf8_names_match_extensions_and_globs_as_raw_bytes() {
    let tree = Tree::new(&[]);
    tree.file(b"bad\xff.TXT");
    for args in [
        vec!["-e", "TXT"],
        vec!["*.TXT"],
        vec!["-ie", "txt"],
        vec!["-i", "bad*.txt"],
    ] {
        let output = tree.run(&args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        let expected = [tree.0.as_os_str().as_bytes(), b"/bad\xff.TXT\n"].concat();
        assert_eq!(output.stdout, expected, "{args:?}");
    }
}

#[test]
fn shell_quoting_preserves_wildcards_and_escapes_with_home_expansion() {
    let tree = Tree::new(&[
        "Documents/a.txt",
        "Documents/nested/b.txt",
        "Documents/report*.rs",
        "Documents/report-one.rs",
    ]);
    // Real shell invocation verifies the documented commands, including tilde expansion.
    for (command, count) in [
        ("\"$FAF_BIN\" -e txt ~/Documents -q --color never", 2),
        ("\"$FAF_BIN\" '*.txt' ~/Documents -q --color never", 2),
        (
            "\"$FAF_BIN\" 'report\\*.rs' ~/Documents -q --color never",
            1,
        ),
    ] {
        let output = Command::new("sh")
            .args(["-c", command])
            .current_dir(&tree.0)
            .env("HOME", &tree.0)
            .env("FAF_BIN", env!("CARGO_BIN_EXE_faf"))
            .env("FAF_LAST", tree.1.join("cache/last"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().lines().count(),
            count,
            "{command}"
        );
    }
}

#[test]
fn existing_nonwildcard_modes_keep_their_semantics() {
    let tree = Tree::new(&["main.rs", "main.go", "domain.rs", "main.rs.bak"]);
    tree.matches(&["main.rs"], &["main.rs", "main.go"]);
    tree.matches(&["-p", "main.rs"], &["main.rs"]);
    tree.matches(
        &["-s", "main"],
        &["main.rs", "main.go", "domain.rs", "main.rs.bak"],
    );
}

#[test]
fn unicode_lowercasing_is_consistent_across_glob_strategies() {
    let tree = Tree::new(&["K-a.txt", "a.K", "a-k.txt"]);
    tree.matches(&["-i", "k*"], &["K-a.txt"]);
    tree.matches(&["-i", "*.k"], &["a.K"]);
    tree.matches(&["-ie", "k"], &["a.K"]);
    tree.matches(&["-i", "*k*"], &["K-a.txt", "a.K", "a-k.txt"]);
    tree.matches(&["-i", "k*a*.txt"], &["K-a.txt"]);
}

#[test]
fn unicode_queries_match_ascii_names_when_lowercasing_allows_it() {
    let tree = Tree::new(&["k-a.txt", "a.k", "a.ü"]);
    tree.matches(&["-i", "K*"], &["k-a.txt"]);
    tree.matches(&["-i", "*.K"], &["a.k"]);
    tree.matches(&["-ie", "K"], &["a.k"]);
    tree.matches(&["-i", "*K*"], &["k-a.txt", "a.k"]);
    tree.matches(&["-i", "*ü*"], &["a.ü"]);
}

#[test]
fn colored_output_highlights_literals_without_changing_filename_bytes() {
    let tree = Tree::new(&["alpha-middle-z.txt"]);
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_faf"))
            .args(args)
            .arg(&tree.0)
            .args(["-q", "--color", "always"])
            .env("FAF_LAST", tree.1.join("cache/last"))
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    };
    let extension = run(&["-e", "txt"]);
    assert!(extension.contains("\x1b[32m.txt\x1b[0m"), "{extension:?}");
    let glob = run(&["alpha*middle*.txt"]);
    for literal in ["alpha", "middle", ".txt"] {
        assert!(
            glob.contains(&format!("\x1b[32m{literal}\x1b[0m")),
            "{glob:?}"
        );
    }
}

#[test]
fn null_output_preserves_newlines_and_disables_color() {
    let tree = Tree::new(&["line\nbreak.txt"]);
    let out = Command::new(env!("CARGO_BIN_EXE_faf"))
        .args(["-0", "*.txt"])
        .arg(&tree.0)
        .args(["-q", "--color", "always"])
        .env("FAF_LAST", tree.1.join("cache/last"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        out.stdout,
        [tree.0.as_os_str().as_bytes(), b"/line\nbreak.txt\0"].concat()
    );
}

#[test]
fn other_glob_metacharacters_are_literal() {
    let tree = Tree::new(&[
        "name?.txt",
        "name1.txt",
        "name[ab].txt",
        "namea.txt",
        "name{a,b}.txt",
    ]);
    tree.matches(&["name?*"], &["name?.txt"]);
    tree.matches(&["name[ab]*"], &["name[ab].txt"]);
    tree.matches(&["name{a,b}*"], &["name{a,b}.txt"]);
}

#[test]
fn ascii_glob_literals_are_highlighted_in_unicode_and_non_utf8_names() {
    let tree = Tree::new(&["über-middle.TXT"]);
    tree.file(b"bad\xff-middle.TXT");
    for args in [vec!["-ie", "txt"], vec!["-i", "*middle*.txt"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_faf"))
            .args(&args)
            .arg(&tree.0)
            .args(["-q", "--color", "always"])
            .env("FAF_LAST", tree.1.join("cache/last"))
            .output()
            .unwrap();
        assert!(out.status.success());
        let green_suffix = b"\x1b[32m.TXT\x1b[0m";
        assert_eq!(
            out.stdout
                .windows(green_suffix.len())
                .filter(|s| *s == green_suffix)
                .count(),
            2,
            "{args:?}: {:?}",
            out.stdout
        );
        if args.contains(&"*middle*.txt") {
            let middle = b"\x1b[32mmiddle\x1b[0m";
            assert_eq!(
                out.stdout
                    .windows(middle.len())
                    .filter(|s| *s == middle)
                    .count(),
                2
            );
        }
    }
}

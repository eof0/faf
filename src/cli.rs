use clap::{Parser, ValueEnum};

use crate::config::EntryType;
use crate::exclude::{ExcludeSpec, NameMode};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

const AFTER_HELP: &str = r#"Matching modes (default: stem - strips extension, exact stem match):
  faf main .               stem: finds main.rs, main.go - not domain.rs
  faf main.rs .            same as faf main (extension on query is ignored)
  faf -s foo /home         substr: finds foobar, foo.txt, prefoo
  faf -p Makefile /etc     exact: full filename must match literally

Other examples:
  faf -i README .          case-insensitive stem match
  faf -d src /home         directories only (same as --type d)
  faf -sd cache /var       substr match, directories only
  faf --max-depth 3 main .
  faf --gitignore src .

Excluding (comma separated, may repeat, may come after ROOT):
  faf main ~ -x target,node_modules     exact name, files and directories
  faf main ~ -x ~/work/typescript,main.lua
                                        a value with / or ~ is a path under ROOT
  faf main ~ -xd build                  directories only (-xf: files only)
  faf main ~ -xs header,/c/             substring of the name, or of the path below ROOT
  faf main ~ -xp main.rs                exact name, spelled explicitly
  Modifiers stack: -xfs, -xds, -xdp. Long forms: --exclude-dir, --exclude-file-substr, ...
"#;

#[derive(Parser, Debug)]
#[command(name = "faf")]
#[command(version)]
#[command(about = "Fast filesystem search by filename")]
#[command(after_help = AFTER_HELP)]
#[command(arg_required_else_help = true)]
pub struct Cli {
    #[arg(short = 's', long)]
    pub substr: bool,

    #[arg(short = 'p', long)]
    pub precise: bool,

    /// Print every scanned file
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Case-insensitive matching
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Filter by entry type: f = files only, d = directories only
    #[arg(long = "type")]
    pub entry_type: Option<String>,

    /// Match files only (alias for --type f)
    #[arg(short = 'f')]
    pub file: bool,

    /// Match directories only (alias for --type d)
    #[arg(short = 'd')]
    pub dir: bool,

    /// Separate output with NUL instead of newline (for xargs -0)
    #[arg(short = '0', long = "null")]
    pub null: bool,

    #[arg(long)]
    pub max_depth: Option<usize>,

    /// Exclude names or paths (comma separated, repeatable; -xp is the same)
    #[arg(
        short = 'x',
        long,
        visible_alias = "exclude-precise",
        value_delimiter = ',',
        value_name = "PATTERNS"
    )]
    pub exclude: Vec<String>,

    /// Exclude files only (-xf)
    #[arg(
        long,
        alias = "exclude-file-precise",
        value_delimiter = ',',
        value_name = "PATTERNS"
    )]
    pub exclude_file: Vec<String>,

    /// Exclude directories only (-xd)
    #[arg(
        long,
        alias = "exclude-dir-precise",
        value_delimiter = ',',
        value_name = "PATTERNS"
    )]
    pub exclude_dir: Vec<String>,

    /// Exclude by substring of the name, or of the path below the root (-xs)
    #[arg(long, value_delimiter = ',', value_name = "PATTERNS")]
    pub exclude_substr: Vec<String>,

    #[arg(long, value_delimiter = ',', hide = true)]
    pub exclude_file_substr: Vec<String>,

    #[arg(long, value_delimiter = ',', hide = true)]
    pub exclude_dir_substr: Vec<String>,

    /// Respect .gitignore files
    #[arg(long)]
    pub gitignore: bool,

    /// Suppress the summary line (scanned/found/elapsed)
    #[arg(short = 'q', long)]
    pub quiet: bool,

    #[arg(long, value_enum, default_value = "auto")]
    pub color: ColorMode,

    #[arg(value_name = "TARGET")]
    pub target: String,

    #[arg(value_name = "ROOT")]
    pub root: Option<PathBuf>,
}

impl Cli {
    /// Takes the six exclude groups out of the parsed arguments, each
    /// tagged with the entry type and name mode its flag stands for.
    pub fn exclude_specs(&mut self) -> Vec<ExcludeSpec> {
        use EntryType::{Any, Dir, File};
        use NameMode::{Precise, Substr};
        [
            (&mut self.exclude, Any, Precise),
            (&mut self.exclude_substr, Any, Substr),
            (&mut self.exclude_file, File, Precise),
            (&mut self.exclude_file_substr, File, Substr),
            (&mut self.exclude_dir, Dir, Precise),
            (&mut self.exclude_dir_substr, Dir, Substr),
        ]
        .into_iter()
        .map(|(patterns, entry_type, mode)| ExcludeSpec {
            patterns: std::mem::take(patterns),
            entry_type,
            mode,
        })
        .collect()
    }
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

/// Expands the `-x` modifier shorthands clap cannot express.
///
/// `-xf`, `-xd`, `-xp`, `-xs` and stacked forms like `-xfs` become the
/// matching `--exclude-*` long option, also when clustered after other
/// short flags (`-sxd` becomes `-s --exclude-dir`) or given with `=`.
/// Only a tail made entirely of modifier letters is rewritten, so `-xfoo`
/// still means `-x foo`. A name that is literally `f`, `d`, `p` or `s`
/// needs a space. Rewriting stops at `--`.
pub fn rewrite_args<I>(args: I) -> Result<Vec<OsString>, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut out = Vec::new();
    let mut literal = false;
    for (i, arg) in args.into_iter().enumerate() {
        // argv[0] is the program path, never a flag.
        if i == 0 || literal || arg == "--" {
            literal |= arg == "--";
            out.push(arg);
            continue;
        }
        match rewrite_cluster(arg.as_bytes())? {
            Some(rewritten) => out.extend(rewritten),
            None => out.push(arg),
        }
    }
    Ok(out)
}

/// Splits `-<flags>x<mods>[=value]` into `-<flags>` and `--exclude-<mods>[=value]`.
fn rewrite_cluster(bytes: &[u8]) -> Result<Option<Vec<OsString>>, String> {
    if !bytes.starts_with(b"-") || bytes.starts_with(b"--") {
        return Ok(None);
    }
    let (head, value) = match memchr::memchr(b'=', bytes) {
        Some(i) => (&bytes[..i], &bytes[i..]),
        None => (bytes, &b""[..]),
    };
    let cluster = &head[1..];
    let Some(x) = memchr::memchr(b'x', cluster) else {
        return Ok(None);
    };
    let (prefix, mods) = (&cluster[..x], &cluster[x + 1..]);
    if mods.is_empty() || !mods.iter().all(|b| b"fdps".contains(b)) {
        return Ok(None);
    }
    let long = exclude_long_name(mods)
        .map_err(|why| format!("{} is not valid: {why}", String::from_utf8_lossy(bytes)))?;
    let mut out = Vec::with_capacity(2);
    if !prefix.is_empty() {
        out.push(OsString::from_vec([b"-", prefix].concat()));
    }
    out.push(OsString::from_vec([long.as_bytes(), value].concat()));
    Ok(Some(out))
}

fn exclude_long_name(mods: &[u8]) -> Result<String, &'static str> {
    let mut kind: Option<&str> = None;
    let mut mode: Option<&str> = None;
    for &m in mods {
        let (slot, word) = match m {
            b'f' => (&mut kind, "file"),
            b'd' => (&mut kind, "dir"),
            b'p' => (&mut mode, "precise"),
            b's' => (&mut mode, "substr"),
            _ => unreachable!("caller filters to fdps"),
        };
        match slot {
            Some(prev) if *prev == word => return Err("repeated modifier"),
            Some(_) if word == "file" || word == "dir" => {
                return Err("cannot combine f and d");
            }
            Some(_) => return Err("cannot combine p and s"),
            None => *slot = Some(word),
        }
    }
    let mut name = String::from("--exclude");
    for part in [kind, mode].into_iter().flatten() {
        name.push('-');
        name.push_str(part);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw(args: &[&str]) -> Result<Vec<String>, String> {
        let argv = std::iter::once("faf").chain(args.iter().copied());
        rewrite_args(argv.map(OsString::from)).map(|v| {
            v.into_iter()
                .skip(1)
                .map(|a| a.into_string().unwrap())
                .collect()
        })
    }

    #[test]
    fn program_name_is_never_rewritten() {
        let out = rewrite_args(["-xf", "-xf"].iter().map(OsString::from)).unwrap();
        assert_eq!(out, ["-xf", "--exclude-file"]);
    }

    #[test]
    fn single_modifiers_map_to_long_options() {
        assert_eq!(rw(&["-xf", "a"]).unwrap(), ["--exclude-file", "a"]);
        assert_eq!(rw(&["-xd", "a"]).unwrap(), ["--exclude-dir", "a"]);
        assert_eq!(rw(&["-xp", "a"]).unwrap(), ["--exclude-precise", "a"]);
        assert_eq!(rw(&["-xs", "a"]).unwrap(), ["--exclude-substr", "a"]);
    }

    #[test]
    fn stacked_modifiers_are_order_independent() {
        assert_eq!(rw(&["-xfs"]).unwrap(), ["--exclude-file-substr"]);
        assert_eq!(rw(&["-xsf"]).unwrap(), ["--exclude-file-substr"]);
        assert_eq!(rw(&["-xdp"]).unwrap(), ["--exclude-dir-precise"]);
    }

    #[test]
    fn bare_x_and_attached_values_are_untouched() {
        assert_eq!(rw(&["-x", "f"]).unwrap(), ["-x", "f"]);
        assert_eq!(rw(&["-xfoo"]).unwrap(), ["-xfoo"]);
        assert_eq!(rw(&["-xf.txt"]).unwrap(), ["-xf.txt"]);
        assert_eq!(rw(&["--exclude"]).unwrap(), ["--exclude"]);
        assert_eq!(rw(&["--exclude-file"]).unwrap(), ["--exclude-file"]);
    }

    #[test]
    fn modifiers_are_split_out_of_a_short_flag_cluster() {
        assert_eq!(rw(&["-sxd", "a"]).unwrap(), ["-s", "--exclude-dir", "a"]);
        assert_eq!(rw(&["-ifxs"]).unwrap(), ["-if", "--exclude-substr"]);
        assert_eq!(rw(&["-sx", "a"]).unwrap(), ["-sx", "a"]);
        assert_eq!(rw(&["-sxfoo"]).unwrap(), ["-sxfoo"]);
    }

    #[test]
    fn equals_form_keeps_its_value() {
        assert_eq!(rw(&["-xf=a,b"]).unwrap(), ["--exclude-file=a,b"]);
        assert_eq!(rw(&["-sxd=a"]).unwrap(), ["-s", "--exclude-dir=a"]);
        assert_eq!(rw(&["-x=f"]).unwrap(), ["-x=f"]);
        assert_eq!(rw(&["-xfoo=bar"]).unwrap(), ["-xfoo=bar"]);
    }

    #[test]
    fn conflicts_and_repeats_are_errors() {
        assert!(rw(&["-xfd"]).unwrap_err().contains("-xfd"));
        assert!(rw(&["-xps"]).is_err());
        assert!(rw(&["-xff"]).is_err());
    }

    #[test]
    fn nothing_after_double_dash_is_rewritten() {
        assert_eq!(rw(&["--", "-xf"]).unwrap(), ["--", "-xf"]);
    }
}

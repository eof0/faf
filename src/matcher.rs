use memchr::memmem::Finder;

use crate::config::MatchMode;

/// NAME_MAX on Linux, macOS and the BSDs, plus one.
pub const NAME_BUF: usize = 256;

#[derive(Clone)]
pub struct MatchTarget {
    /// Prebuilt searcher over the normalized query. Building one per call
    /// costs more than the search itself on short filenames.
    pub(crate) finder: Finder<'static>,
    pub(crate) mode: MatchMode,
    pub(crate) ignore_case: bool,
    /// Query is pure ASCII, so byte-wise case folding is exact.
    pub(crate) ascii: bool,
    glob: Option<Box<Glob>>,
}

impl MatchTarget {
    /// Compile search syntax once. Exclusion names use `new` and stay literal.
    pub fn search(raw: &str, mode: MatchMode, ignore_case: bool) -> Result<Self, String> {
        if !raw.as_bytes().iter().any(|b| matches!(b, b'*' | b'\\')) {
            return if mode == MatchMode::Extension {
                Self::extension(raw, ignore_case)
            } else {
                Ok(Self::new(raw, mode, ignore_case))
            };
        }

        // Split only on unescaped stars. Each segment remains valid UTF-8.
        let mut segments = vec![String::new()];
        let mut chars = raw.chars();
        let mut wildcard = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let escaped = chars
                        .next()
                        .ok_or("trailing backslash in target; use \\\\ for a literal backslash")?;
                    segments.last_mut().unwrap().push(escaped);
                }
                '*' => {
                    wildcard = true;
                    segments.push(String::new());
                }
                _ => segments.last_mut().unwrap().push(c),
            }
        }

        if !wildcard {
            let literal = &segments[0];
            return if mode == MatchMode::Extension {
                Self::extension(literal, ignore_case)
            } else {
                // An escaped default target names a full filename, not a stem.
                let mode = if mode == MatchMode::Standard {
                    MatchMode::Precise
                } else {
                    mode
                };
                Ok(Self::new(literal, mode, ignore_case))
            };
        }
        if mode != MatchMode::Standard {
            let flag = match mode {
                MatchMode::Substr => "-s",
                MatchMode::Precise => "-p",
                MatchMode::Extension => "-e",
                _ => unreachable!("search only accepts CLI matching modes"),
            };
            return Err(format!(
                "wildcard targets cannot be combined with {flag}; use the quoted wildcard target alone"
            ));
        }

        let prefix = !segments[0].is_empty();
        let suffix = !segments.last().unwrap().is_empty();
        let mut literals: Vec<_> = segments.into_iter().filter(|s| !s.is_empty()).collect();
        let ascii = literals.iter().all(|s| s.is_ascii());
        let simple_mode = match (literals.len(), prefix, suffix) {
            (0, _, _) => Some(MatchMode::All),
            (1, true, false) => Some(MatchMode::Prefix),
            (1, false, true) => Some(MatchMode::Suffix),
            (1, false, false) => Some(MatchMode::GlobContains),
            _ => None,
        };
        if let Some(mode) = simple_mode {
            return Ok(Self::new(
                literals.first().map_or("", String::as_str),
                mode,
                ignore_case,
            ));
        }
        if ignore_case {
            for literal in &mut literals {
                *literal = literal.to_lowercase();
            }
        }
        let min_len = literals.iter().map(String::len).sum();
        let end = if suffix {
            literals.pop().unwrap().into_bytes()
        } else {
            Vec::new()
        };
        let mut literals = literals.into_iter();
        let start = if prefix {
            literals.next().unwrap().into_bytes()
        } else {
            Vec::new()
        };
        let middle = literals
            .map(|s| Finder::new(s.as_bytes()).into_owned())
            .collect();
        let mut target = Self::new("", MatchMode::Glob, ignore_case);
        target.ascii = ascii;
        target.glob = Some(Box::new(Glob {
            prefix: start.into_boxed_slice(),
            suffix: end.into_boxed_slice(),
            middle,
            min_len,
        }));
        Ok(target)
    }

    fn extension(raw: &str, ignore_case: bool) -> Result<Self, String> {
        let ext = raw.strip_prefix('.').unwrap_or(raw);
        if ext.is_empty() || ext.as_bytes().iter().any(|b| matches!(b, b'/' | b'\0')) {
            return Err("extension must be nonempty and cannot contain / or NUL".to_owned());
        }
        Ok(Self::new(
            &format!(".{ext}"),
            MatchMode::Extension,
            ignore_case,
        ))
    }

    pub fn new(raw: &str, mode: MatchMode, ignore_case: bool) -> Self {
        let effective = if mode == MatchMode::Standard {
            // stem_bytes splits at an ASCII dot, so the slice stays valid UTF-8.
            std::str::from_utf8(stem_bytes(raw.as_bytes())).unwrap_or(raw)
        } else {
            raw
        };
        let needle = if ignore_case {
            effective.to_lowercase()
        } else {
            effective.to_owned()
        };
        Self {
            finder: Finder::new(needle.as_bytes()).into_owned(),
            mode,
            ignore_case,
            ascii: effective.is_ascii(),
            glob: None,
        }
    }

    #[inline(always)]
    pub fn needle(&self) -> &[u8] {
        self.finder.needle()
    }

    #[inline(always)]
    pub fn is_match(&self, name: &[u8]) -> bool {
        match self.mode {
            MatchMode::Substr => self.contains(name),
            MatchMode::Precise => self.equals(name),
            MatchMode::Standard => self.equals(stem_bytes(name)),
            // Keep the original three modes' dispatch small. Pattern matching
            // has its own dispatch, reached only when a new mode was selected.
            _ => self.matches_pattern(name),
        }
    }

    #[inline(never)]
    fn matches_pattern(&self, name: &[u8]) -> bool {
        match self.mode {
            MatchMode::Extension | MatchMode::Suffix => self.ends_with(name),
            MatchMode::Prefix => self.starts_with(name),
            MatchMode::Glob => self.matches_glob(name),
            MatchMode::GlobContains => {
                if !self.ignore_case {
                    self.finder.find(name).is_some()
                } else {
                    folded_match(name, self.ascii, |s| self.finder.find(s).is_some())
                }
            }
            MatchMode::All => true,
            _ => unreachable!("literal modes are handled by is_match"),
        }
    }

    #[inline(always)]
    pub(crate) fn equals(&self, name: &[u8]) -> bool {
        let needle = self.needle();
        if !self.ignore_case {
            return name == needle;
        }
        if self.ascii {
            return name.eq_ignore_ascii_case(needle);
        }
        unicode_lower(name).is_some_and(|l| l.as_bytes() == needle)
    }

    #[inline(always)]
    pub(crate) fn contains(&self, name: &[u8]) -> bool {
        if !self.ignore_case {
            return self.finder.find(name).is_some();
        }
        if self.ascii && name.is_ascii() {
            let mut buf = [0u8; NAME_BUF];
            if let Some(lower) = ascii_lower(name, &mut buf) {
                return self.finder.find(lower).is_some();
            }
        }
        unicode_lower(name).is_some_and(|l| self.finder.find(l.as_bytes()).is_some())
    }

    #[inline(always)]
    fn starts_with(&self, name: &[u8]) -> bool {
        if !self.ignore_case {
            return name.starts_with(self.needle());
        }
        if self.ascii {
            let matches = name
                .get(..self.needle().len())
                .is_some_and(|s| s.eq_ignore_ascii_case(self.needle()));
            if matches || name.is_ascii() || std::str::from_utf8(name).is_err() {
                return matches;
            }
        }
        folded_match(name, self.ascii, |s| s.starts_with(self.needle()))
    }

    #[inline(always)]
    fn ends_with(&self, name: &[u8]) -> bool {
        if !self.ignore_case {
            return name.ends_with(self.needle());
        }
        if self.ascii {
            let matches = name
                .len()
                .checked_sub(self.needle().len())
                .is_some_and(|at| name[at..].eq_ignore_ascii_case(self.needle()));
            if matches || name.is_ascii() || std::str::from_utf8(name).is_err() {
                return matches;
            }
        }
        folded_match(name, self.ascii, |s| s.ends_with(self.needle()))
    }

    fn matches_glob(&self, name: &[u8]) -> bool {
        let glob = self.glob.as_ref().unwrap();
        if !self.ignore_case {
            return glob.is_match(name);
        }
        folded_match(name, self.ascii, |s| glob.is_match(s))
    }

    /// Visit matching literal spans for terminal output only, after a match.
    /// Folding Unicode may change byte offsets, so that case has no inner spans.
    pub(crate) fn glob_spans(&self, name: &[u8], mut visit: impl FnMut(usize, usize)) {
        let mut buf = [0u8; NAME_BUF];
        let hay = if !self.ignore_case {
            name
        } else if self.ascii {
            let Some(lower) = ascii_lower(name, &mut buf) else {
                return;
            };
            lower
        } else {
            return;
        };
        match self.mode {
            MatchMode::Extension | MatchMode::Suffix => {
                if hay.ends_with(self.needle()) {
                    visit(name.len() - self.needle().len(), name.len());
                }
            }
            MatchMode::Prefix => {
                if hay.starts_with(self.needle()) {
                    visit(0, self.needle().len());
                }
            }
            MatchMode::Glob => {
                let glob = self.glob.as_ref().unwrap();
                // ASCII folding preserves byte offsets even in Unicode or
                // invalid UTF-8 names. If full Unicode folding was needed to
                // match, skip inner spans rather than guess their offsets.
                if !glob.is_match(hay) {
                    return;
                }
                if !glob.prefix.is_empty() {
                    visit(0, glob.prefix.len());
                }
                let mut at = glob.prefix.len();
                let end = hay.len() - glob.suffix.len();
                for segment in &glob.middle {
                    let Some(found) = segment.find(&hay[at..end]) else {
                        return;
                    };
                    at += found;
                    visit(at, at + segment.needle().len());
                    at += segment.needle().len();
                }
                if !glob.suffix.is_empty() {
                    visit(end, hay.len());
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
struct Glob {
    prefix: Box<[u8]>,
    suffix: Box<[u8]>,
    middle: Vec<Finder<'static>>,
    min_len: usize,
}

impl Glob {
    fn is_match(&self, name: &[u8]) -> bool {
        if name.len() < self.min_len
            || !name.starts_with(&self.prefix)
            || !name.ends_with(&self.suffix)
        {
            return false;
        }
        // Reserve the anchored suffix before scanning. Earliest middle hits
        // always leave the most room for the remaining segments; no backtracking.
        let mut rest = &name[self.prefix.len()..name.len() - self.suffix.len()];
        for segment in &self.middle {
            let Some(at) = segment.find(rest) else {
                return false;
            };
            rest = &rest[at + segment.needle().len()..];
        }
        true
    }
}

/// Fold ASCII names on the stack, regardless of the query's encoding.
/// Valid Unicode names retain the existing Unicode lowercase semantics.
fn folded_match(name: &[u8], ascii: bool, matches: impl FnOnce(&[u8]) -> bool) -> bool {
    if name.is_ascii() || (ascii && std::str::from_utf8(name).is_err()) {
        let mut buf = [0u8; NAME_BUF];
        return ascii_lower(name, &mut buf).is_some_and(matches);
    }
    unicode_lower(name).is_some_and(|s| matches(s.as_bytes()))
}

/// ASCII-lowercase `name` into `buf`. None if it does not fit.
#[inline(always)]
pub fn ascii_lower<'a>(name: &[u8], buf: &'a mut [u8; NAME_BUF]) -> Option<&'a [u8]> {
    let out = buf.get_mut(..name.len())?;
    for (dst, &src) in out.iter_mut().zip(name) {
        *dst = src.to_ascii_lowercase();
    }
    Some(out)
}

#[cold]
fn unicode_lower(name: &[u8]) -> Option<String> {
    std::str::from_utf8(name).ok().map(str::to_lowercase)
}

/// Filename without its extension. A leading dot is part of the stem.
#[inline(always)]
pub fn stem_bytes(bytes: &[u8]) -> &[u8] {
    match memchr::memrchr(b'.', bytes) {
        Some(i) if i > 0 => &bytes[..i],
        _ => bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(mode: MatchMode, target: &str, filename: &str) -> bool {
        MatchTarget::new(target, mode, false).is_match(filename.as_bytes())
    }

    fn matches_i(mode: MatchMode, target: &str, filename: &str) -> bool {
        MatchTarget::new(target, mode, true).is_match(filename.as_bytes())
    }

    #[test]
    fn standard_stem_query_with_extension_matches_stem() {
        assert!(matches(MatchMode::Standard, "walker.rs", "walker.rs"));
        assert!(matches(MatchMode::Standard, "walker.rs", "walker.go"));
        assert!(!matches(MatchMode::Standard, "walker.rs", "mywalker.rs"));
    }

    #[test]
    fn standard_stem_query_without_extension_same_as_with() {
        assert_eq!(
            matches(MatchMode::Standard, "walker", "walker.rs"),
            matches(MatchMode::Standard, "walker.rs", "walker.rs"),
        );
    }

    #[test]
    fn precise_requires_full_filename() {
        assert!(matches(MatchMode::Precise, "walker.rs", "walker.rs"));
        assert!(!matches(MatchMode::Precise, "walker", "walker.rs"));
        assert!(!matches(MatchMode::Precise, "walker.rs", "walker.go"));
    }

    #[test]
    fn hidden_files_keep_leading_dot_in_stem() {
        assert!(matches(MatchMode::Standard, ".bashrc", ".bashrc"));
        assert!(!matches(MatchMode::Standard, "bashrc", ".bashrc"));
        assert_eq!(stem_bytes(b".config.json"), b".config");
    }

    #[test]
    fn ignore_case_ascii() {
        assert!(matches_i(MatchMode::Standard, "README", "readme.md"));
        assert!(matches_i(MatchMode::Precise, "makefile", "Makefile"));
        assert!(matches_i(MatchMode::Substr, "LIB", "zlib.so"));
        assert!(!matches_i(MatchMode::Substr, "LIB", "zlob.so"));
    }

    #[test]
    fn ignore_case_unicode() {
        assert!(matches_i(MatchMode::Standard, "Über", "über.txt"));
        assert!(matches_i(MatchMode::Standard, "über", "ÜBER.txt"));
        assert!(matches_i(MatchMode::Substr, "ÜBER", "xüberx"));
        assert!(matches_i(MatchMode::Substr, "lib", "zlib_ü.so"));
        assert!(!matches_i(MatchMode::Precise, "über", "uber"));
    }

    #[test]
    fn substr_matches_in_extension() {
        assert!(matches(MatchMode::Substr, "txt", "foo.txt"));
        assert!(matches(MatchMode::Substr, "o.t", "foo.txt"));
    }

    #[test]
    fn compiled_globs_agree_with_an_independent_wildcard_reference() {
        // The reference enumerates all possible star lengths rather than
        // reusing the production matcher's ordered segment algorithm.
        fn reference(pattern: &[u8], name: &[u8]) -> bool {
            match pattern.split_first() {
                None => name.is_empty(),
                Some((b'*', tail)) => (0..=name.len()).any(|at| reference(tail, &name[at..])),
                Some((literal, tail)) => name
                    .split_first()
                    .is_some_and(|(c, rest)| c == literal && reference(tail, rest)),
            }
        }
        fn words(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
            let mut all = vec![Vec::new()];
            let mut level = all.clone();
            for _ in 0..max_len {
                let mut next = Vec::new();
                for word in &level {
                    for &byte in alphabet {
                        let mut word = word.clone();
                        word.push(byte);
                        next.push(word);
                    }
                }
                all.extend(next.iter().cloned());
                level = next;
            }
            all
        }
        let names = words(b"ab", 6);
        for pattern in words(b"ab*", 6) {
            if !pattern.contains(&b'*') {
                continue;
            }
            let query = std::str::from_utf8(&pattern).unwrap();
            let target = MatchTarget::search(query, MatchMode::Standard, false).unwrap();
            for name in &names {
                assert_eq!(
                    target.is_match(name),
                    reference(&pattern, name),
                    "pattern={query:?}, name={name:?}"
                );
            }
        }
    }

    #[test]
    fn globs_handle_maximum_length_filenames_and_longer_patterns() {
        let name = [b'a'; NAME_BUF - 1];
        for query in ["a*", "*a", "*a*", "a*a*a"] {
            assert!(
                MatchTarget::search(query, MatchMode::Standard, true)
                    .unwrap()
                    .is_match(&name)
            );
        }
        let too_long = format!("{}*a", "a".repeat(NAME_BUF));
        assert!(
            !MatchTarget::search(&too_long, MatchMode::Standard, false)
                .unwrap()
                .is_match(&name)
        );
    }
}

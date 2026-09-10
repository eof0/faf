use memchr::memmem::Finder;

use crate::config::MatchMode;

/// NAME_MAX on Linux, macOS and the BSDs, plus one.
pub const NAME_BUF: usize = 256;

pub struct MatchTarget {
    /// Prebuilt searcher over the normalized query. Building one per call
    /// costs more than the search itself on short filenames.
    pub(crate) finder: Finder<'static>,
    pub(crate) mode: MatchMode,
    pub(crate) ignore_case: bool,
    /// Query is pure ASCII, so byte-wise case folding is exact.
    pub(crate) ascii: bool,
}

impl MatchTarget {
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
        }
    }

    #[inline(always)]
    fn equals(&self, name: &[u8]) -> bool {
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
    fn contains(&self, name: &[u8]) -> bool {
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
}

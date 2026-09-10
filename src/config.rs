use crate::matcher::MatchTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Substr,
    Precise,
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Any,
    File,
    Dir,
}

pub struct WalkConfig {
    pub target: MatchTarget,
    pub max_depth: Option<usize>,
    /// Excluded directory names. A linear scan beats hashing at this size.
    pub exclude: Vec<Box<[u8]>>,
    pub entry_type: EntryType,
    pub null_terminate: bool,
    pub gitignore: bool,
    pub verbose: bool,
    pub color: bool,
}

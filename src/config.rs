use crate::exclude::ExcludeSet;
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
    pub exclude: ExcludeSet,
    pub entry_type: EntryType,
    pub null_terminate: bool,
    pub gitignore: bool,
    pub verbose: bool,
    pub color: bool,
}

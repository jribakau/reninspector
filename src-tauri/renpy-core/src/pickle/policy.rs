//! What a pickle is allowed to contain, and how large it may grow.

/// How globals (`GLOBAL`, `STACK_GLOBAL`, `REDUCE`, `BUILD`, `NEWOBJ`) are handled.
/// Nothing here imports or calls a class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Globals {
    /// Only `_codecs.encode` may be called. Objects, sets and protocol-0 floats are rejected.
    /// This is an archive index.
    EncodeOnly,
    /// Modules a Ren'Py script pickle uses. A global becomes an inert object.
    Allowlist,
    /// Any global becomes an inert object. Unknown list and dict subclasses keep their items.
    /// This is a save or persistent file.
    Inert,
}

/// Caps for one `load` call. Counters live on the parser, not here.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub max_objects: usize,
    pub max_bytes: usize,
    /// Values produced while freezing, plus containers copied during `REDUCE` and `BUILD`.
    pub max_values: usize,
    pub max_output_bytes: usize,
    /// Reject the input before parsing when set.
    pub max_input: Option<usize>,
}

const SCRIPT_LIMITS: Limits = Limits {
    max_objects: 5_000_000,
    max_bytes: 256 * 1024 * 1024,
    max_values: 10_000_000,
    max_output_bytes: 256 * 1024 * 1024,
    max_input: None,
};

/// An index entry costs up to six values (name, list, tuple, two numbers, a
/// prefix), and `MAX_ENTRIES` allows two million entries. The object cap
/// still bounds the real allocation, so this only has to clear that.
const ARCHIVE_LIMITS: Limits = Limits {
    max_values: 20_000_000,
    ..SCRIPT_LIMITS
};

const SAVE_LIMITS: Limits = Limits {
    max_objects: 200_000,
    max_bytes: 32 * 1024 * 1024,
    max_values: 2_000_000,
    max_output_bytes: 32 * 1024 * 1024,
    max_input: Some(32 * 1024 * 1024),
};

/// Which pickles `load` will accept.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub(crate) globals: Globals,
    pub(crate) limits: Limits,
}

impl Policy {
    /// RPA index. The only callable global is `_codecs.encode`.
    pub const ARCHIVE_INDEX: Policy = Policy {
        globals: Globals::EncodeOnly,
        limits: ARCHIVE_LIMITS,
    };

    /// Compiled `.rpyc` statement tree.
    pub const SCRIPT: Policy = Policy {
        globals: Globals::Allowlist,
        limits: SCRIPT_LIMITS,
    };

    /// Save slot or persistent data. Tighter size limits than a script.
    pub const SAVE: Policy = Policy {
        globals: Globals::Inert,
        limits: SAVE_LIMITS,
    };

    pub(crate) fn encode_only(self) -> bool {
        matches!(self.globals, Globals::EncodeOnly)
    }

    pub(crate) fn inert(self) -> bool {
        matches!(self.globals, Globals::Inert)
    }

    /// Script and save pickles point `next` back at an ancestor. That edge is cut.
    /// An archive index has no reason to be cyclic, so a cycle is an error there.
    pub(crate) fn cut_cycles(self) -> bool {
        !self.encode_only()
    }
}

//! Ren'Py archive (`.rpa` / `.rpi`) reader and writer.
//!
//! The index is a Python pickle. [`pickle`] accepts only the opcodes an index
//! needs, so nothing in an archive can run code. Writing always produces
//! RPA-3.0 with a protocol-2 index, which both Ren'Py 7 and Ren'Py 8 load.
//! The game's own archives are never modified by this module; callers write a
//! new file (a patch archive, an extract folder, or an archive the user asked
//! to build).

mod extract;
mod format;
mod patch;
mod pickle;
mod reader;
mod writer;
mod zix;

pub use extract::{extract, ExtractOptions, ExtractReport};
pub use format::{ArchiveVersion, Entry, RpaError, Segment};
pub use patch::{
    check_stale, format_utc, patch_stem, read_manifest, utc_now, PatchEntry, PatchManifest,
    MANIFEST_NAME,
};
pub use pickle::{dump_index, IndexPair};
pub use reader::{Archive, CopyStats};
pub use writer::{build_from_dir, ArchiveWriter, BuildReport};

/// Bytes of one entry kept in memory. Previews stay under this; scripts may be larger.
pub const PREVIEW_MAX: u64 = 8 * 1024 * 1024;
pub const SCRIPT_MAX: u64 = 64 * 1024 * 1024;

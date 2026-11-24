//! Entry points for the fuzz targets in `fuzz/`. The crate builds this module
//! only with `--cfg fuzzing`, which `cargo fuzz` sets.

use std::path::Path;

/// Parses command-line arguments, as `logbrew` does at startup.
#[must_use]
pub fn command_line(args: &[String]) -> bool {
    crate::parse_command(args).is_ok()
}

/// Parses `bytes` as one ELF, Mach-O, or universal Mach-O debug file and
/// returns how many debug artifacts it holds.
#[must_use]
pub fn debug_file(bytes: &[u8]) -> Option<usize> {
    crate::native_debug_artifacts::fuzz_debug_file(bytes)
}

/// Reads the file at `path` as a symbol ZIP and returns how many debug
/// artifacts it holds. The path must end in `.zip`.
#[must_use]
pub fn symbol_zip(path: &Path) -> Option<usize> {
    crate::native_debug_artifacts::fuzz_symbol_zip(path)
}

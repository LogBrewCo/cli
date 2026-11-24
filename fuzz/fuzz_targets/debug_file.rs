//! Fuzzes the ELF and Mach-O debug file parser.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _artifacts = logbrew_cli::fuzzing::debug_file(data);
});

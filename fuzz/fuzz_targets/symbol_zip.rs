//! Fuzzes the symbol ZIP reader with each input saved as a `.zip` file.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;
use std::sync::OnceLock;

/// One file per fuzzing process, rewritten for every input.
fn zip_path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::temp_dir().join(format!("logbrew-fuzz-{}.zip", std::process::id()))
    })
}

fuzz_target!(|data: &[u8]| {
    let path = zip_path();
    if std::fs::write(path, data).is_ok() {
        let _artifacts = logbrew_cli::fuzzing::symbol_zip(path);
    }
});

//! Fuzzes the command-line parser with arguments split on NUL bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let args = std::iter::once("logbrew".to_owned())
        .chain(
            data.split(|byte| *byte == 0)
                .map(|arg| String::from_utf8_lossy(arg).into_owned()),
        )
        .collect::<Vec<_>>();
    let _parsed = logbrew_cli::fuzzing::command_line(&args);
});

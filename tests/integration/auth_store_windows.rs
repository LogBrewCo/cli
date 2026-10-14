//! Compile the actual credential permission helpers for Windows.

use crate::TestResult;
use std::fmt::Write as _;

#[test]
fn windows_permission_helpers_compile_and_reject_unused_parameters() -> TestResult {
    let source = include_str!("../../src/auth/store.rs");
    let mut fixture = String::from("#![deny(unused)]\npub type RuntimeError = std::io::Error;\n");
    for name in ["secure_file_permissions", "secure_path_permissions"] {
        let signature = format!("fn {name}(");
        let (_, body) = source
            .split_once(&signature)
            .ok_or("missing permission helper")?;
        let (body, _) = body
            .split_once("\n}")
            .ok_or("unterminated permission helper")?;
        writeln!(fixture, "pub {signature}{body}\n}}")?;
    }
    let root = super::isolated_home("logbrew-windows-auth-store", "compile")?;
    let path = root.join("auth_store.rs");
    let result = (|| -> TestResult {
        for (text, valid) in [
            (fixture.clone(), true),
            (
                fixture
                    .replace("let _ = file;", "")
                    .replace("let _ = path;", ""),
                false,
            ),
        ] {
            std::fs::write(&path, text)?;
            let output = super::run_bounded(
                std::process::Command::new("rustc")
                    .args([
                        "--crate-type=lib",
                        "--edition=2024",
                        "--emit=metadata",
                        "--deny=warnings",
                    ])
                    .arg("--out-dir")
                    .arg(&root)
                    .args(["--target", "x86_64-pc-windows-msvc"])
                    .arg(&path),
            )?;
            assert_eq!(
                output.status.success(),
                valid,
                "Windows permission compile contract failed"
            );
            if !valid {
                let diagnostics = std::str::from_utf8(&output.stderr)?;
                for expected in ["unused variable: `file`", "unused variable: `path`"] {
                    assert!(
                        diagnostics.contains(expected),
                        "missing parameter diagnostic"
                    );
                }
            }
        }
        Ok(())
    })();
    std::fs::remove_dir_all(root)?;
    result
}

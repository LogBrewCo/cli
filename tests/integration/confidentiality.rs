//! Native confidentiality checks with path-free, non-reflecting failures.

use std::io::Read as _;
use std::path::Path;

const SECRET_PATTERN: &str = "AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY|GOOGLE_APPLICATION_CREDENTIALS|DATABASE_URL=|JWT_SECRET=|PASSWORD=|SECRET_KEY=|PRIVATE_KEY|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer [A-Za-z0-9._=-]{20,}|ghp_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]+|sk-[A-Za-z0-9]{20,}";
const DENYLIST: &str = ".logbrew-confidential-denylist.local";

/// Preserve scan status, but never propagate matched text or regex diagnostics.
fn scan(
    root: &Path,
    patterns: impl IntoIterator<Item = impl AsRef<str>>,
    path: &Path,
) -> Result<(), &'static str> {
    let mut command = std::process::Command::new("rg");
    let _command =
        command
            .current_dir(root)
            .args(["--no-config", "--quiet", "--no-messages", "--hidden"]);
    if path == Path::new(".") {
        let _command = command.args([
            "--glob",
            "!target/**",
            "--glob",
            "!.git/**",
            "--glob",
            "!tests/integration/confidentiality.rs",
            "--glob",
            "!.logbrew-confidential-denylist.local",
        ]);
    } else {
        let _command = command.args(["--ignore-case", "--text"]);
    }
    let mut patterns = patterns.into_iter().peekable();
    if patterns.peek().is_none() {
        return Ok(());
    }
    for pattern in patterns {
        let _command = command.arg("--regexp").arg(pattern.as_ref());
    }
    let output = super::run_bounded(command.arg("--").arg(path))
        .map_err(|_| "confidentiality scanner unavailable or timed out")?;
    match output.status.code() {
        Some(1) => Ok(()),
        Some(0) => Err("confidentiality match"),
        _ => Err("confidentiality scanner failed"),
    }
}

fn check(root: &Path) -> crate::TestResult {
    scan(root, [SECRET_PATTERN], Path::new("."))?;
    scan(root, [r"\bprivate\b"], Path::new("README.md"))?;
    let mut fixtures = 0_u32;
    for entry in std::fs::read_dir(root.join("tests/fixtures"))? {
        let entry = entry?;
        if entry.file_name().to_str().is_some_and(|name| {
            name.starts_with("native_elf")
                && Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("so"))
        }) {
            scan(root, ["/Users/|/home/runner/|/private/var/"], &entry.path())?;
            fixtures += 1;
        }
    }
    if fixtures == 0 {
        return Err("native confidentiality fixtures missing".into());
    }
    match std::fs::symlink_metadata(root.join(DENYLIST)) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err("denylist must be a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("denylist unavailable".into()),
    }
    let mut content = String::new();
    let size = std::fs::File::open(root.join(DENYLIST))
        .map_err(|_| "denylist unavailable")?
        .take(65_537)
        .read_to_string(&mut content)
        .map_err(|_| "denylist could not be read")?;
    if size > 65_536 {
        return Err("denylist exceeded its limit".into());
    }
    let patterns = content
        .split('\n')
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    scan(root, patterns, Path::new("."))?;
    Ok(())
}

#[test]
fn repository_contains_no_confidential_material() -> crate::TestResult {
    check(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// Exercise every retained default rule using synthetic, non-secret markers.
fn check_secret_markers(root: &Path) -> crate::TestResult {
    let source = root.join("source.txt");
    for content in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "DATABASE_URL=",
        "JWT_SECRET=",
        "PASSWORD=",
        "SECRET_KEY=",
        "PRIVATE_KEY",
        "-----BEGIN RSA PRIVATE KEY-----",
        "Bearer synthetic-token-000000000000",
        "ghp_synthetic000000000000000000",
        "github_pat_synthetic",
        "sk-synthetic0000000000000000",
    ] {
        std::fs::write(&source, content)?;
        assert_eq!(
            scan(root, [SECRET_PATTERN], Path::new(".")),
            Err("confidentiality match"),
            "missed synthetic marker"
        );
    }
    Ok(())
}

#[test]
fn confidentiality_rules_and_local_patterns_fail_closed() -> crate::TestResult {
    let root = super::isolated_home("logbrew-confidentiality", "rules")?;
    let result = (|| -> crate::TestResult {
        std::fs::create_dir_all(root.join("tests/fixtures"))?;
        std::fs::write(root.join("README.md"), "public CLI")?;
        std::fs::write(root.join("tests/fixtures/native_elf.so"), b"ELF\0public")?;
        check(&root)?;
        let source = root.join("source.txt");
        check_secret_markers(&root)?;
        std::fs::write(&source, "ordinary public diagnostic")?;
        for (path, content, expected) in [
            (
                "README.md",
                "PRIVATE details",
                Some("confidentiality match"),
            ),
            ("README.md", "privateer", None),
            (
                "tests/fixtures/native_elf.so",
                "ELF\0/Users/synthetic",
                Some("confidentiality match"),
            ),
            (
                "tests/fixtures/native_elf.so",
                "ELF\0/HOME/RUNNER/synthetic",
                Some("confidentiality match"),
            ),
            (
                "tests/fixtures/native_elf.so",
                "ELF\0/private/var/synthetic",
                Some("confidentiality match"),
            ),
        ] {
            let path = root.join(path);
            let saved = std::fs::read(&path)?;
            std::fs::write(&path, content)?;
            assert_eq!(
                check(&root).err().map(|error| error.to_string()).as_deref(),
                expected,
                "file policy drifted"
            );
            std::fs::write(path, saved)?;
        }
        for (pattern, expected) in [
            ("ordinary", Err("confidentiality match")),
            ("--files", Ok(())),
            ("[", Err("confidentiality scanner failed")),
        ] {
            assert_eq!(
                scan(&root, [pattern], Path::new(".")),
                expected,
                "incorrect scan outcome"
            );
        }
        assert_eq!(
            scan(&root, ["anything"], Path::new("missing.txt")),
            Err("confidentiality scanner failed"),
            "missing file passed"
        );
        for (pattern, expected) in [
            ("# comments only\n \n", None),
            ("# comment\n \nnever-matches", None),
            ("never-matches\nordinary", Some("confidentiality match")),
            ("[", Some("confidentiality scanner failed")),
            ("ordinary\n[", Some("confidentiality scanner failed")),
            ("[\nordinary", Some("confidentiality scanner failed")),
            ("ordinary\r", None),
            ("--files", None),
        ] {
            std::fs::write(root.join(DENYLIST), pattern)?;
            assert_eq!(
                check(&root).err().map(|error| error.to_string()).as_deref(),
                expected,
                "denylist outcome drifted"
            );
        }
        std::fs::File::create(root.join(DENYLIST))?.set_len(65_537)?;
        assert!(check(&root).is_err(), "oversized denylist passed");
        std::fs::write(root.join(DENYLIST), [0xff])?;
        assert!(check(&root).is_err(), "non-UTF-8 denylist passed");
        std::fs::remove_file(root.join(DENYLIST))?;
        std::fs::create_dir(root.join(DENYLIST))?;
        assert!(check(&root).is_err(), "directory denylist passed");
        Ok(())
    })();
    std::fs::remove_dir_all(root)?;
    result
}

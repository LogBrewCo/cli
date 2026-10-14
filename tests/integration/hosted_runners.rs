//! Enforce the repository's supported GitHub-hosted runner policy.

use std::path::Path;

const RUNNERS: &[&str] = &[
    "macos-14",
    "macos-15",
    "macos-15-intel",
    "macos-latest",
    "ubuntu-22.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04",
    "ubuntu-24.04-arm",
    "ubuntu-latest",
    "windows-2022",
    "windows-2025",
    "windows-latest",
];
const MATRIX: &str = "${{ matrix.runner }}";
const WINDOWS: &str = "github-custom-runners = { \"x86_64-pc-windows-msvc\" = \"windows-2025\" }";

fn is_workflow(path: &Path) -> bool {
    path.parent() == Some(Path::new(".github/workflows"))
        && path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.starts_with('y') && extension.ends_with("ml"))
}

/// Reject unsupported policy without reflecting source contents.
fn validate(path: &Path, content: &str) -> Result<(), &'static str> {
    if content.to_lowercase().contains(concat!("black", "smith")) {
        return Err("unsupported runner provider");
    }
    if path == Path::new("dist-workspace.toml")
        && (content.matches("github-custom-runners").count() != 1 || !content.contains(WINDOWS))
    {
        return Err("distribution must use the official Windows 2025 runner");
    }
    if is_workflow(path) {
        for line in content.lines() {
            let Some((key, value)) = line.trim_start().split_once(':') else {
                continue;
            };
            if !matches!(key.trim(), "runs-on" | "runner") {
                continue;
            }
            let value = value.split('#').next().unwrap_or_default().trim();
            let runner = match value.chars().next() {
                Some(quote @ ('\'' | '"')) => value
                    .strip_prefix(quote)
                    .and_then(|inner| inner.strip_suffix(quote))
                    .ok_or("unpaired runner quote")?,
                _ => value,
            };
            if key != key.trim()
                || !(RUNNERS.contains(&runner) || key == "runs-on" && runner == MATRIX)
            {
                return Err("unsupported runner declaration");
            }
        }
    }
    Ok(())
}

#[test]
fn repository_uses_supported_hosted_runners() -> crate::TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = super::run_bounded(std::process::Command::new("git").current_dir(root).args([
        "ls-files",
        "-z",
        "--",
        "README.md",
        "dist-workspace.toml",
        ".github",
        "docs",
        "scripts",
    ]))?;
    assert!(output.status.success(), "runner policy inventory failed");
    let mut paths = std::str::from_utf8(&output.stdout)?
        .split_terminator('\0')
        .map(std::path::PathBuf::from)
        .collect::<std::collections::BTreeSet<_>>();
    for entry in std::fs::read_dir(root.join(".github/workflows"))? {
        let path = entry?.path();
        let relative = path.strip_prefix(root)?;
        if is_workflow(relative) {
            let _inserted = paths.insert(relative.to_owned());
        }
    }
    assert!(
        paths.contains(Path::new("dist-workspace.toml")),
        "missing distribution policy"
    );
    for path in paths {
        let content = std::fs::read_to_string(root.join(&path))?;
        validate(&path, &content)?;
    }
    Ok(())
}

#[test]
fn runner_labels_quotes_comments_and_matrices_are_validated() -> crate::TestResult {
    let path = Path::new(".github/workflows/fixture.yaml");
    for runner in RUNNERS.iter().copied().chain([MATRIX]) {
        for value in [
            runner.to_owned(),
            format!("'{runner}'"),
            format!("\"{runner}\""),
        ] {
            validate(path, &format!("  runs-on: {value} # note"))?;
        }
    }
    for value in [
        "",
        "# absent",
        "self-hosted",
        "[ubuntu-latest]",
        "${{ inputs.runner }}",
        "'ubuntu-latest\"",
        "\"ubuntu-latest'",
        "ubuntu-latest extra",
        "|",
    ] {
        assert!(
            validate(path, &format!("runs-on: {value}")).is_err(),
            "invalid runner accepted"
        );
    }
    for value in [
        "runs-on : ubuntu-latest",
        "runner: self-hosted",
        "runner: ${{ matrix.runner }}",
    ] {
        assert!(
            validate(path, value).is_err(),
            "invalid declaration accepted"
        );
    }
    validate(path, "runner: windows-2025")?;
    validate(path, "name: test\n# runs-on: ignored")?;
    Ok(())
}

#[test]
fn distribution_and_provider_policy_fail_closed() -> crate::TestResult {
    let path = Path::new("dist-workspace.toml");
    validate(path, WINDOWS)?;
    for content in [
        String::new(),
        format!("{WINDOWS}\n{WINDOWS}"),
        WINDOWS.replace("windows-2025", "windows-2022"),
    ] {
        assert!(
            validate(path, &content).is_err(),
            "invalid distribution accepted"
        );
    }
    assert!(
        validate(Path::new("README.md"), concat!("BLACK", "SMITH")).is_err(),
        "provider bypass"
    );
    Ok(())
}

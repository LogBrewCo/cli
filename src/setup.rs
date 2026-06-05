//! Local SDK setup planning.

use std::path::Path;

/// Maximum directory depth scanned for nearby project manifests.
const MAX_SCAN_DEPTH: usize = 3;
/// Maximum parent levels checked when setup is run from a project subdirectory.
const MAX_PARENT_SCAN_DEPTH: usize = 3;
/// Next step when setup finds a supported project.
const SDK_NEXT_STEP: &str = "install the matching LogBrew SDK package when packages are ready; \
                             send release and environment with logs, issues, actions, and traces";
/// Next step when setup cannot find a supported project.
const EMPTY_NEXT_STEP: &str = "run logbrew setup from a project containing package.json, \
                               pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
                               project.yaml, .xcodeproj, .xcworkspace, go.mod, or composer.json.";

/// Writes the non-mutating setup plan.
pub(crate) fn write_setup_plan<W: std::io::Write>(
    root: Option<&Path>,
    auto: bool,
    yes: bool,
    json: bool,
    output: &mut W,
) -> Result<(), std::io::Error> {
    let root = root.unwrap_or_else(|| Path::new("."));
    let plan = SetupPlan::detect(root, auto, yes);

    if json {
        let detected = plan
            .detected
            .iter()
            .map(|detection| {
                serde_json::json!({
                    "runtime": detection.runtime,
                    "package_manager": detection.package_manager,
                    "manifest": detection.manifest,
                })
            })
            .collect::<Vec<_>>();
        let body = serde_json::json!({
            "ok": true,
            "auto": plan.auto,
            "yes": plan.yes,
            "install_ready": false,
            "detected": detected,
            "next": plan.next_step(),
        });
        return writeln!(output, "{body}");
    }

    writeln!(output, "LogBrew setup plan")?;
    writeln!(output, "Mode: non-mutating plan")?;
    if plan.auto || plan.yes {
        writeln!(output, "Preferences: auto={}, yes={}", plan.auto, plan.yes)?;
    }
    writeln!(output, "No files changed.")?;
    writeln!(output, "Install: not ready")?;
    if plan.detected.is_empty() {
        writeln!(output, "No supported project manifest found.")?;
    } else {
        writeln!(output, "Detected runtimes:")?;
        for detection in &plan.detected {
            writeln!(
                output,
                "- {} ({}) at {}",
                display_runtime(detection.runtime),
                detection.package_manager,
                detection.manifest
            )?;
        }
    }
    writeln!(output, "Next: {}", plan.next_step())
}

/// Non-mutating SDK setup plan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupPlan {
    /// Whether automatic setup was requested.
    auto: bool,
    /// Whether confirmation prompts should be skipped.
    yes: bool,
    /// Detected project manifests, at most one per runtime.
    detected: Vec<ProjectDetection>,
}

impl SetupPlan {
    /// Builds a setup plan by scanning the project root.
    fn detect(root: &Path, auto: bool, yes: bool) -> Self {
        Self {
            auto,
            yes,
            detected: detect_projects(root),
        }
    }

    /// Returns the setup follow-up step.
    fn next_step(&self) -> &'static str {
        if self.detected.is_empty() {
            EMPTY_NEXT_STEP
        } else {
            SDK_NEXT_STEP
        }
    }
}

/// One detected project manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectDetection {
    /// Stable runtime key.
    runtime: &'static str,
    /// Package manager or ecosystem used by the runtime.
    package_manager: &'static str,
    /// Manifest path relative to the scanned root.
    manifest: String,
}

/// Detects supported project manifests under a root.
fn detect_projects(root: &Path) -> Vec<ProjectDetection> {
    let mut detected = Vec::new();
    collect_manifests(root, root, 0, &mut detected);
    if detected.is_empty() {
        collect_parent_manifests(root, &mut detected);
    }
    detected.sort_by(|left, right| {
        manifest_depth(left.manifest.as_str())
            .cmp(&manifest_depth(right.manifest.as_str()))
            .then_with(|| left.runtime.cmp(right.runtime))
            .then_with(|| {
                manifest_priority(left.manifest.as_str())
                    .cmp(&manifest_priority(right.manifest.as_str()))
            })
            .then_with(|| left.manifest.cmp(&right.manifest))
    });
    dedupe_by_runtime(detected)
}

/// Collects project manifests from nearby parent directories.
fn collect_parent_manifests(root: &Path, detected: &mut Vec<ProjectDetection>) {
    let mut current = root;
    for _ in 0..MAX_PARENT_SCAN_DEPTH {
        let Some(parent) = current.parent() else {
            return;
        };
        collect_direct_manifests(root, parent, detected);
        if !detected.is_empty() {
            return;
        }
        current = parent;
    }
}

/// Collects supported manifest entries directly inside one directory.
fn collect_direct_manifests(root: &Path, directory: &Path, detected: &mut Vec<ProjectDetection>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        if let Some(detection) = manifest_detection(root, entry.path().as_path()) {
            detected.push(detection);
        }
    }
}

/// Recursively collects supported manifests.
fn collect_manifests(
    root: &Path,
    directory: &Path,
    depth: usize,
    detected: &mut Vec<ProjectDetection>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if let Some(detection) = manifest_detection(root, path.as_path()) {
            detected.push(detection);
            if file_type.is_dir() {
                continue;
            }
        }

        if file_type.is_dir() && depth < MAX_SCAN_DEPTH && !should_skip_dir(path.as_path()) {
            collect_manifests(root, path.as_path(), depth + 1, detected);
        }
    }
}

/// Builds a project manifest detection when a path is a supported manifest.
fn manifest_detection(root: &Path, path: &Path) -> Option<ProjectDetection> {
    let (runtime, package_manager) = manifest_runtime(path)?;
    Some(ProjectDetection {
        runtime,
        package_manager,
        manifest: relative_manifest(root, path),
    })
}

/// Returns whether a directory should be skipped during setup detection.
fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| {
            matches!(
                name,
                ".build"
                    | ".git"
                    | ".swiftpm"
                    | ".venv"
                    | "DerivedData"
                    | "node_modules"
                    | "target"
                    | "vendor"
                    | "venv"
            )
        })
}

/// Maps a manifest path to a runtime and package manager.
fn manifest_runtime(path: &Path) -> Option<(&'static str, &'static str)> {
    let file_name = path.file_name().and_then(std::ffi::OsStr::to_str)?;
    match file_name {
        "Cargo.toml" => Some(("rust", "cargo")),
        "Package.swift" => Some(("swift", "swift package manager")),
        "composer.json" => Some(("php", "composer")),
        "go.mod" => Some(("go", "go")),
        "package.json" => Some(("node", node_package_manager(path))),
        "Pipfile" => Some(("python", "pipenv")),
        "project.yml" | "project.yaml" => Some(("swift-ios", "xcodegen")),
        "pyproject.toml" => Some(("python", python_package_manager(path))),
        _ if file_name.ends_with(".xcodeproj") => Some(("swift-ios", "xcode")),
        _ if file_name.ends_with(".xcworkspace") => Some(("swift-ios", "xcode workspace")),
        _ => None,
    }
}

/// Detects the Node package manager from sibling lockfiles.
fn node_package_manager(package_json: &Path) -> &'static str {
    let Some(directory) = package_json.parent() else {
        return "npm";
    };
    if directory.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if directory.join("yarn.lock").exists() {
        "yarn"
    } else if directory.join("bun.lockb").exists() || directory.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    }
}

/// Detects the Python package manager from sibling lockfiles.
fn python_package_manager(pyproject: &Path) -> &'static str {
    let Some(directory) = pyproject.parent() else {
        return "pip";
    };
    if directory.join("uv.lock").exists() {
        "uv"
    } else if directory.join("poetry.lock").exists() {
        "poetry"
    } else if directory.join("Pipfile.lock").exists() || directory.join("Pipfile").exists() {
        "pipenv"
    } else {
        "pip"
    }
}

/// Returns a manifest path relative to the project root.
fn relative_manifest(root: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return display_path(relative);
    }
    relative_path(root, path).unwrap_or_else(|| display_path(path))
}

/// Returns a portable display path with forward slashes.
fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// Builds a relative path from the setup directory to an ancestor manifest.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let root_components = root.components().collect::<Vec<_>>();
    let path_components = path.components().collect::<Vec<_>>();
    let common = root_components
        .iter()
        .zip(path_components.iter())
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }

    let mut parts = Vec::new();
    for _ in common..root_components.len() {
        parts.push(String::from(".."));
    }
    for component in &path_components[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }

    if parts.is_empty() {
        Some(String::from("."))
    } else {
        Some(parts.join("/"))
    }
}

/// Returns an approximate path depth for nearest-manifest sorting.
fn manifest_depth(path: &str) -> usize {
    path.split('/').count()
}

/// Returns the source-of-truth preference when several manifests describe one runtime.
fn manifest_priority(path: &str) -> usize {
    if matches!(path, "project.yml" | "project.yaml")
        || path.ends_with("/project.yml")
        || path.ends_with("/project.yaml")
    {
        0
    } else if path.ends_with(".xcworkspace") {
        1
    } else if path.ends_with(".xcodeproj") {
        2
    } else {
        3
    }
}

/// Keeps the nearest manifest for each runtime.
fn dedupe_by_runtime(detected: Vec<ProjectDetection>) -> Vec<ProjectDetection> {
    let mut runtimes = Vec::new();
    let mut deduped = Vec::new();
    for detection in detected {
        if runtimes.contains(&detection.runtime) {
            continue;
        }
        runtimes.push(detection.runtime);
        deduped.push(detection);
    }
    deduped
}

/// Returns human-readable runtime names.
fn display_runtime(runtime: &str) -> &'static str {
    match runtime {
        "go" => "Go",
        "node" => "Node",
        "php" => "PHP",
        "python" => "Python",
        "rust" => "Rust",
        "swift" => "Swift",
        "swift-ios" => "Swift/iOS",
        _ => "Project",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{ProjectDetection, detect_projects};

    #[test]
    fn detects_nearest_manifest_per_runtime() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("nearest")?;
        fs::write(root.join("Cargo.toml"), "")?;
        fs::create_dir_all(root.join("crates/logbrew"))?;
        fs::write(root.join("crates/logbrew/Cargo.toml"), "")?;
        fs::write(root.join("package.json"), "{}")?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![
                ProjectDetection {
                    runtime: "node",
                    package_manager: "npm",
                    manifest: String::from("package.json"),
                },
                ProjectDetection {
                    runtime: "rust",
                    package_manager: "cargo",
                    manifest: String::from("Cargo.toml"),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn detects_node_package_manager_from_lockfile() -> Result<(), Box<dyn std::error::Error>> {
        for (lockfile, package_manager) in [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lockb", "bun"),
            ("package-lock.json", "npm"),
        ] {
            let root = fixture(lockfile)?;
            fs::write(root.join("package.json"), "{}")?;
            fs::write(root.join(lockfile), "")?;

            let detected = detect_projects(root.as_path());

            assert_eq!(
                detected,
                vec![ProjectDetection {
                    runtime: "node",
                    package_manager,
                    manifest: String::from("package.json"),
                }]
            );
        }
        Ok(())
    }

    #[test]
    fn detects_python_package_manager_from_lockfile() -> Result<(), Box<dyn std::error::Error>> {
        for (lockfile, package_manager) in [
            ("uv.lock", "uv"),
            ("poetry.lock", "poetry"),
            ("Pipfile.lock", "pipenv"),
        ] {
            let root = fixture(lockfile)?;
            fs::write(root.join("pyproject.toml"), "")?;
            fs::write(root.join(lockfile), "")?;

            let detected = detect_projects(root.as_path());

            assert_eq!(
                detected,
                vec![ProjectDetection {
                    runtime: "python",
                    package_manager,
                    manifest: String::from("pyproject.toml"),
                }]
            );
        }
        Ok(())
    }

    #[test]
    fn detects_pipfile_as_python_project() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("pipfile")?;
        fs::write(root.join("Pipfile"), "")?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![ProjectDetection {
                runtime: "python",
                package_manager: "pipenv",
                manifest: String::from("Pipfile"),
            }]
        );
        Ok(())
    }

    #[test]
    fn detects_xcodegen_ios_project_manifest() -> Result<(), Box<dyn std::error::Error>> {
        for manifest in ["project.yml", "project.yaml"] {
            let root = fixture(manifest)?;
            fs::write(root.join(manifest), "name: Checkout\n")?;

            let detected = detect_projects(root.as_path());

            assert_eq!(
                detected,
                vec![ProjectDetection {
                    runtime: "swift-ios",
                    package_manager: "xcodegen",
                    manifest: String::from(manifest),
                }]
            );
        }
        Ok(())
    }

    #[test]
    fn detects_xcode_project_directories() -> Result<(), Box<dyn std::error::Error>> {
        for (manifest, package_manager) in [
            ("Checkout.xcodeproj", "xcode"),
            ("Checkout.xcworkspace", "xcode workspace"),
        ] {
            let root = fixture(manifest)?;
            fs::create_dir_all(root.join(manifest))?;

            let detected = detect_projects(root.as_path());

            assert_eq!(
                detected,
                vec![ProjectDetection {
                    runtime: "swift-ios",
                    package_manager,
                    manifest: String::from(manifest),
                }]
            );
        }
        Ok(())
    }

    #[test]
    fn prefers_xcodegen_manifest_over_generated_xcode_containers()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("xcodegen-preference")?;
        fs::write(root.join("project.yaml"), "name: Checkout\n")?;
        fs::create_dir_all(root.join("Checkout.xcodeproj"))?;
        fs::create_dir_all(root.join("Checkout.xcworkspace"))?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![ProjectDetection {
                runtime: "swift-ios",
                package_manager: "xcodegen",
                manifest: String::from("project.yaml"),
            }]
        );
        Ok(())
    }

    fn fixture(name: &str) -> Result<PathBuf, std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "logbrew-cli-setup-module-{name}-{}",
            std::process::id()
        ));
        remove_dir_if_exists(root.as_path())?;
        fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn remove_dir_if_exists(path: &std::path::Path) -> Result<(), std::io::Error> {
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

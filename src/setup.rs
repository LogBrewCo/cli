//! Local SDK setup planning.

use std::path::Path;

/// Maximum directory depth scanned for nearby project manifests.
const MAX_SCAN_DEPTH: usize = 3;
/// Maximum parent levels checked when setup is run from a project subdirectory.
const MAX_PARENT_SCAN_DEPTH: usize = 3;
/// Next step when setup finds a supported project.
const SDK_NEXT_STEP: &str = "use the released SDK guidance for this runtime; this CLI version does \
                             not yet provide a structured install plan";
/// Next step when a public Python package plan is available.
const PYTHON_NEXT_STEP: &str =
    "review the compatibility requirements, then run the install command; no files were changed";
/// Minimum Python version required by the current public Python SDK family.
const PYTHON_MINIMUM_VERSION: &str = ">=3.11";
/// Minimum Django version required by the current public Django integration.
const DJANGO_MINIMUM_VERSION: &str = "Django>=5.2";
/// Minimum Flask version required by the current public Flask integration.
const FLASK_MINIMUM_VERSION: &str = "Flask>=3.1";
/// Minimum `FastAPI` version required by the current public `FastAPI` integration.
const FASTAPI_MINIMUM_VERSION: &str = "FastAPI>=0.111.1";
/// Maximum bytes read from any manifest while detecting a Python framework.
const MAX_FRAMEWORK_MANIFEST_BYTES: u64 = 256 * 1024;
/// Public Swift package URL used by a non-mutating install plan.
const SWIFT_PACKAGE_URL: &str = "https://github.com/LogBrewCo/sdk.git";
/// Public Swift package product consumed by application targets.
const SWIFT_PRODUCT: &str = "LogBrew";
/// Minimum public Swift package release required by this setup plan.
const SWIFT_MINIMUM_VERSION: &str = "0.1.6";
/// `SwiftPM` requirement that accepts compatible releases before version 1.0.0.
const SWIFT_VERSION_REQUIREMENT: &str = "up_to_next_major";
/// Next step when a public Swift package can be planned truthfully.
const SWIFT_NEXT_STEP: &str =
    "add the LogBrew Swift package from the install plan; no files were changed";
/// Next step when setup cannot find a supported project.
const EMPTY_NEXT_STEP: &str = "run logbrew setup from a project containing package.json, \
                               pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
                               project.yaml, .xcodeproj, .xcworkspace, go.mod, or composer.json.";

/// Released Python integration selected from local project evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PythonIntegration {
    /// Framework-neutral Python client.
    Core,
    /// Django request and exception middleware.
    Django,
    /// Flask request and exception middleware.
    Flask,
    /// `FastAPI` request and exception middleware.
    FastApi,
}

impl PythonIntegration {
    /// Returns the stable machine key for the integration.
    const fn key(self) -> &'static str {
        match self {
            Self::Core => "python",
            Self::Django => "django",
            Self::Flask => "flask",
            Self::FastApi => "fastapi",
        }
    }

    /// Returns the human-readable integration name.
    const fn display_name(self) -> &'static str {
        match self {
            Self::Core => "Python",
            Self::Django => "Django",
            Self::Flask => "Flask",
            Self::FastApi => "FastAPI",
        }
    }

    /// Returns the public packages and their roles.
    const fn packages(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Core => &[("logbrew-sdk", "core")],
            Self::Django => &[("logbrew-sdk", "core"), ("logbrew-django", "framework")],
            Self::Flask => &[("logbrew-sdk", "core"), ("logbrew-flask", "framework")],
            Self::FastApi => &[("logbrew-sdk", "core"), ("logbrew-fastapi", "framework")],
        }
    }

    /// Returns the released framework compatibility requirement, when applicable.
    const fn framework_requirement(self) -> Option<&'static str> {
        match self {
            Self::Core => None,
            Self::Django => Some(DJANGO_MINIMUM_VERSION),
            Self::Flask => Some(FLASK_MINIMUM_VERSION),
            Self::FastApi => Some(FASTAPI_MINIMUM_VERSION),
        }
    }
}

/// A truthful, non-mutating install plan for one released SDK family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallPlan {
    /// Swift Package Manager plan.
    Swift,
    /// Python package-index plan.
    Python {
        /// Detected Python package manager.
        package_manager: &'static str,
        /// Detected framework integration.
        integration: PythonIntegration,
    },
}

impl InstallPlan {
    /// Builds the stable JSON representation.
    fn json(self) -> serde_json::Value {
        match self {
            Self::Swift => serde_json::json!({
                "mode": "non_mutating",
                "ecosystem": "swiftpm",
                "package_url": SWIFT_PACKAGE_URL,
                "product": SWIFT_PRODUCT,
                "version": SWIFT_MINIMUM_VERSION,
                "version_requirement": {
                    "kind": SWIFT_VERSION_REQUIREMENT,
                    "minimum_version": SWIFT_MINIMUM_VERSION,
                },
                "dependency_declaration": swift_dependency_declaration(),
                "next_action": {
                    "code": "add_swift_package_dependency",
                    "target": "project_manifest",
                }
            }),
            Self::Python {
                package_manager,
                integration,
            } => {
                let packages = integration
                    .packages()
                    .iter()
                    .map(|(name, role)| {
                        serde_json::json!({
                            "name": name,
                            "role": role,
                            "version_requirement": {
                                "kind": "latest_compatible",
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "mode": "non_mutating",
                    "ecosystem": "pypi",
                    "package_manager": package_manager,
                    "integration": integration.key(),
                    "packages": packages,
                    "compatibility": {
                        "status": "review_required",
                        "requires_python": PYTHON_MINIMUM_VERSION,
                        "requires_framework": integration.framework_requirement(),
                    },
                    "install_command": python_install_command(package_manager, integration),
                    "next_action": {
                        "code": "review_compatibility_and_install",
                        "target": "project_environment",
                    }
                })
            }
        }
    }

    /// Writes the human-readable package details.
    fn write_human<W: std::io::Write>(self, output: &mut W) -> Result<(), std::io::Error> {
        match self {
            Self::Swift => {
                writeln!(output, "Package: {SWIFT_PACKAGE_URL}")?;
                writeln!(output, "Product: {SWIFT_PRODUCT}")?;
                writeln!(output, "Minimum version: {SWIFT_MINIMUM_VERSION}")?;
                writeln!(output, "Dependency: {}", swift_dependency_declaration())
            }
            Self::Python {
                package_manager,
                integration,
            } => {
                let package_names = python_package_names(integration);
                writeln!(output, "Package manager: {package_manager}")?;
                writeln!(output, "Integration: {}", integration.display_name())?;
                writeln!(output, "Packages: {package_names}")?;
                if let Some(requirement) = integration.framework_requirement() {
                    writeln!(
                        output,
                        "Compatibility review: Python {PYTHON_MINIMUM_VERSION}; {requirement}"
                    )?;
                } else {
                    writeln!(
                        output,
                        "Compatibility review: Python {PYTHON_MINIMUM_VERSION}"
                    )?;
                }
                writeln!(
                    output,
                    "Command: {}",
                    python_install_command(package_manager, integration)
                )
            }
        }
    }

    /// Returns the safe next action after the plan is displayed.
    const fn next_step(self) -> &'static str {
        match self {
            Self::Swift => SWIFT_NEXT_STEP,
            Self::Python { .. } => PYTHON_NEXT_STEP,
        }
    }
}

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
    let install_plan = plan.install_plan();

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
            "install_ready": install_plan.is_some(),
            "install_plan": install_plan.map(InstallPlan::json),
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
    if let Some(install_plan) = install_plan {
        writeln!(output, "Install: ready")?;
        install_plan.write_human(output)?;
    } else {
        writeln!(output, "Install: not ready")?;
    }
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

/// Builds the copyable `SwiftPM` dependency declaration from canonical fields.
fn swift_dependency_declaration() -> String {
    format!(r#".package(url: "{SWIFT_PACKAGE_URL}", from: "{SWIFT_MINIMUM_VERSION}")"#)
}

/// Returns the space-separated public Python package names.
fn python_package_names(integration: PythonIntegration) -> String {
    integration
        .packages()
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Builds a copyable package-manager command from public package names.
fn python_install_command(package_manager: &str, integration: PythonIntegration) -> String {
    let packages = python_package_names(integration);
    match package_manager {
        "uv" => format!("uv add {packages}"),
        "poetry" => format!("poetry add {packages}"),
        "pipenv" => format!("pipenv install {packages}"),
        _ => format!("python3 -m pip install --upgrade {packages}"),
    }
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
        } else if let Some(install_plan) = self.install_plan() {
            install_plan.next_step()
        } else {
            SDK_NEXT_STEP
        }
    }

    /// Returns the highest-priority released install plan.
    fn install_plan(&self) -> Option<InstallPlan> {
        if self.detected.iter().any(|detection| {
            matches!(
                detection.package_manager,
                "swift package manager" | "xcodegen"
            )
        }) {
            return Some(InstallPlan::Swift);
        }

        self.detected
            .iter()
            .find(|detection| detection.runtime == "python")
            .map(|detection| InstallPlan::Python {
                package_manager: detection.package_manager,
                integration: detection
                    .python_integration
                    .unwrap_or(PythonIntegration::Core),
            })
    }
}

/// One detected project manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectDetection {
    /// Stable runtime key.
    runtime: &'static str,
    /// Package manager or ecosystem used by the runtime.
    package_manager: &'static str,
    /// Released Python integration inferred from bounded local metadata.
    python_integration: Option<PythonIntegration>,
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
    let file_type = std::fs::symlink_metadata(path).ok()?.file_type();
    let is_xcode_container = matches!(package_manager, "xcode" | "xcode workspace");
    if (is_xcode_container && !file_type.is_dir()) || (!is_xcode_container && !file_type.is_file())
    {
        return None;
    }
    Some(ProjectDetection {
        runtime,
        package_manager,
        python_integration: (runtime == "python").then(|| detect_python_integration(path)),
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

/// Detects a released Python framework integration from bounded project metadata.
fn detect_python_integration(manifest: &Path) -> PythonIntegration {
    let Some(directory) = manifest.parent() else {
        return PythonIntegration::Core;
    };
    if is_regular_file(directory.join("manage.py").as_path()) {
        return PythonIntegration::Django;
    }

    let mut text = read_framework_manifest(manifest).unwrap_or_default();
    for file_name in [
        "requirements.txt",
        "requirements.in",
        "setup.cfg",
        "setup.py",
    ] {
        let candidate = directory.join(file_name);
        if candidate == manifest {
            continue;
        }
        if let Some(candidate_text) = read_framework_manifest(candidate.as_path()) {
            text.push('\n');
            text.push_str(candidate_text.as_str());
        }
    }

    if mentions_python_distribution(text.as_str(), "django") {
        PythonIntegration::Django
    } else if mentions_python_distribution(text.as_str(), "fastapi") {
        PythonIntegration::FastApi
    } else if mentions_python_distribution(text.as_str(), "flask") {
        PythonIntegration::Flask
    } else {
        PythonIntegration::Core
    }
}

/// Reads one small UTF-8 manifest without accepting oversized metadata.
fn read_framework_manifest(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_FRAMEWORK_MANIFEST_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    (u64::try_from(text.len()).ok()? <= MAX_FRAMEWORK_MANIFEST_BYTES).then_some(text)
}

/// Returns whether a path is a regular file rather than a symlink.
fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Returns whether metadata contains a complete normalized distribution token.
fn mentions_python_distribution(text: &str, distribution: &str) -> bool {
    text.split(|character: char| {
        !character.is_ascii_alphanumeric() && character != '-' && character != '_'
    })
    .any(|token| token.eq_ignore_ascii_case(distribution))
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

    use super::{ProjectDetection, PythonIntegration, detect_projects};

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
                    python_integration: None,
                    manifest: String::from("package.json"),
                },
                ProjectDetection {
                    runtime: "rust",
                    package_manager: "cargo",
                    python_integration: None,
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
                    python_integration: None,
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
                    python_integration: Some(PythonIntegration::Core),
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
                python_integration: Some(PythonIntegration::Core),
                manifest: String::from("Pipfile"),
            }]
        );
        Ok(())
    }

    #[test]
    fn detects_django_from_dynamic_requirements() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("django-dynamic-requirements")?;
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"fixture\"\ndynamic = [\"dependencies\"]\n",
        )?;
        fs::write(root.join("requirements.txt"), "Django>=4.2,<6\n")?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![ProjectDetection {
                runtime: "python",
                package_manager: "pip",
                python_integration: Some(PythonIntegration::Django),
                manifest: String::from("pyproject.toml"),
            }]
        );
        Ok(())
    }

    #[test]
    fn oversized_python_metadata_falls_back_to_the_core_plan()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("oversized-python-metadata")?;
        fs::write(root.join("pyproject.toml"), "Django".repeat(50_000))?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![ProjectDetection {
                runtime: "python",
                package_manager: "pip",
                python_integration: Some(PythonIntegration::Core),
                manifest: String::from("pyproject.toml"),
            }]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn framework_detection_does_not_follow_metadata_symlinks()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("python-metadata-symlink")?;
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"fixture\"\n",
        )?;
        let outside = root.with_extension("outside-requirements");
        fs::write(outside.as_path(), "Django>=5.2\n")?;
        std::os::unix::fs::symlink(outside.as_path(), root.join("requirements.txt"))?;

        let detected = detect_projects(root.as_path());

        assert_eq!(
            detected,
            vec![ProjectDetection {
                runtime: "python",
                package_manager: "pip",
                python_integration: Some(PythonIntegration::Core),
                manifest: String::from("pyproject.toml"),
            }]
        );
        fs::remove_file(outside)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn project_detection_rejects_symlinked_manifests() -> Result<(), Box<dyn std::error::Error>> {
        let root = fixture("project-manifest-symlink")?;
        let outside = root.with_extension("outside-pyproject");
        fs::write(
            outside.as_path(),
            "[project]\nname = \"fixture\"\ndependencies = [\"Django\"]\n",
        )?;
        std::os::unix::fs::symlink(outside.as_path(), root.join("pyproject.toml"))?;

        let detected = detect_projects(root.as_path());

        assert!(detected.is_empty());
        fs::remove_file(outside)?;
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
                    python_integration: None,
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
                    python_integration: None,
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
                python_integration: None,
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

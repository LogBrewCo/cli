//! Local SDK setup planning.

use std::{collections::HashSet, path::Path};

/// Maximum directory depth scanned for nearby project manifests.
const MAX_SCAN_DEPTH: usize = 3;
/// Maximum parent levels checked when setup is run from a project subdirectory.
const MAX_PARENT_SCAN_DEPTH: usize = 3;
/// Maximum entries inspected when classifying an `XcodeGen` project's sources.
const MAX_APPLE_SOURCE_SCAN_ENTRIES: usize = 4_096;
/// Next step when setup finds a supported project.
const SDK_NEXT_STEP: &str = "use the released SDK guidance for this runtime; this CLI version does \
                             not yet provide a structured install plan";
/// Next step when a public Python package plan is available.
const PYTHON_NEXT_STEP: &str =
    "review the compatibility requirements, then run the install command; no files were changed";
/// Minimum Python version required by the current public Python SDK family.
const PYTHON_MINIMUM_VERSION: &str = ">=3.10";
/// Supported Django range for the current public Django integration.
const DJANGO_VERSION_REQUIREMENT: &str = "Django>=4.2.30,<6";
/// Minimum Flask version required by the current public Flask integration.
const FLASK_MINIMUM_VERSION: &str = "Flask>=3.1";
/// Minimum `FastAPI` version required by the current public `FastAPI` integration.
const FASTAPI_MINIMUM_VERSION: &str = "FastAPI>=0.111.1";
/// Maximum bytes read from any manifest while detecting a Python framework.
const MAX_FRAMEWORK_MANIFEST_BYTES: u64 = 256 * 1024;
/// Public SDK repository used by non-mutating install plans.
const SDK_PACKAGE_URL: &str = "https://github.com/LogBrewCo/sdk.git";
/// Public Swift package product consumed by application targets.
const SWIFT_PRODUCT: &str = "LogBrew";
/// Minimum public Swift package release required by this setup plan.
const SWIFT_MINIMUM_VERSION: &str = "0.1.6";
/// `SwiftPM` requirement that accepts compatible releases before version 1.0.0.
const SWIFT_VERSION_REQUIREMENT: &str = "up_to_next_major";
/// Next step when a public Swift package can be planned truthfully.
const SWIFT_NEXT_STEP: &str =
    "add the LogBrew Swift package from the install plan; no files were changed";
/// Scoped immutable release tag for the current Objective-C SDK package.
const OBJC_RELEASE_TAG: &str = "objc/logbrew-objc/v0.2.3";
/// Current released Objective-C SDK version.
const OBJC_VERSION: &str = "0.2.3";
/// Objective-C package location inside the public SDK repository.
const OBJC_SOURCE_SUBDIRECTORY: &str = "objc/logbrew-objc";
/// Next step for the released Objective-C source/header package.
const OBJC_NEXT_STEP: &str = "vendor the pinned LogBrew Objective-C header and source directory into the application target; no files were changed";
/// Scoped immutable release tag for the current C++ SDK package.
const CPP_RELEASE_TAG: &str = "cpp/logbrew-cpp/v0.2.3";
/// Current released C++ SDK version.
const CPP_VERSION: &str = "0.2.3";
/// C++ package location inside the public SDK repository.
const CPP_SOURCE_SUBDIRECTORY: &str = "cpp/logbrew-cpp";
/// Exported dependency-free `CMake` target.
const CPP_CORE_TARGET: &str = "LogBrew::LogBrew";
/// Exported optional libcurl `CMake` target.
const CPP_HTTP_TARGET: &str = "LogBrew::HttpTransport";
/// `CMake` option enabling the libcurl transport target.
const CPP_HTTP_OPTION: &str = "LOGBREW_BUILD_HTTP_TRANSPORT";
/// Next step when a released `CMake` package can be planned truthfully.
const CMAKE_NEXT_STEP: &str = "add the pinned LogBrew C++ FetchContent block and link the required target; no files were changed";
/// Next step when setup cannot find a supported project.
const EMPTY_NEXT_STEP: &str = "run logbrew setup from a project containing package.json, \
                               pyproject.toml, Pipfile, Cargo.toml, Package.swift, project.yml, \
                               project.yaml, .xcodeproj, .xcworkspace, CMakeLists.txt, go.mod, or \
                               composer.json.";

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
    /// Returns stable key, display name, and optional framework package requirement.
    const fn details(
        self,
    ) -> (
        &'static str,
        &'static str,
        Option<(&'static str, &'static str)>,
    ) {
        match self {
            Self::Core => ("python", "Python", None),
            Self::Django => (
                "django",
                "Django",
                Some(("logbrew-django", DJANGO_VERSION_REQUIREMENT)),
            ),
            Self::Flask => (
                "flask",
                "Flask",
                Some(("logbrew-flask", FLASK_MINIMUM_VERSION)),
            ),
            Self::FastApi => (
                "fastapi",
                "FastAPI",
                Some(("logbrew-fastapi", FASTAPI_MINIMUM_VERSION)),
            ),
        }
    }
}

/// A truthful, non-mutating install plan for one released SDK family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallPlan {
    /// Swift Package Manager plan.
    Swift,
    /// Source/header plan for the Objective-C SDK.
    ObjectiveC,
    /// `CMake` `FetchContent` plan for the C++ SDK.
    Cmake,
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
                "package_url": SDK_PACKAGE_URL,
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
            Self::ObjectiveC => serde_json::json!({
                "mode": "non_mutating",
                "ecosystem": "source",
                "language": "objective-c",
                "package_url": SDK_PACKAGE_URL,
                "release_tag": OBJC_RELEASE_TAG,
                "version": OBJC_VERSION,
                "source_subdirectory": OBJC_SOURCE_SUBDIRECTORY,
                "header": "include/LogBrew.h",
                "source_directory": "src",
                "frameworks": ["Foundation"],
                "next_action": {
                    "code": "vendor_objective_c_sources",
                    "target": "application_target",
                }
            }),
            Self::Cmake => serde_json::json!({
                "mode": "non_mutating",
                "ecosystem": "cmake",
                "package_url": SDK_PACKAGE_URL,
                "release_tag": CPP_RELEASE_TAG,
                "version": CPP_VERSION,
                "source_subdirectory": CPP_SOURCE_SUBDIRECTORY,
                "targets": {
                    "core": CPP_CORE_TARGET,
                    "http_transport": CPP_HTTP_TARGET,
                },
                "http_transport": {
                    "cmake_option": CPP_HTTP_OPTION,
                    "default_enabled": false,
                    "requires": "libcurl",
                },
                "dependency_declaration": cmake_dependency_declaration(),
                "next_action": {
                    "code": "add_cmake_fetch_content",
                    "target": "CMakeLists.txt",
                }
            }),
            Self::Python {
                package_manager,
                integration,
            } => {
                let (key, _, framework) = integration.details();
                let packages = std::iter::once(("logbrew-sdk", "core"))
                    .chain(framework.map(|(name, _)| (name, "framework")))
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
                    "integration": key,
                    "packages": packages,
                    "compatibility": {
                        "status": "review_required",
                        "requires_python": PYTHON_MINIMUM_VERSION,
                        "requires_framework": framework.map(|(_, requirement)| requirement),
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
            Self::Swift => writeln!(
                output,
                "Package: {SDK_PACKAGE_URL}\nProduct: {SWIFT_PRODUCT}\nMinimum version: \
                 {SWIFT_MINIMUM_VERSION}\nDependency: {}",
                swift_dependency_declaration()
            ),
            Self::ObjectiveC => writeln!(
                output,
                "Package: {SDK_PACKAGE_URL}\nRelease tag: {OBJC_RELEASE_TAG}\nSource subdirectory: \
                 {OBJC_SOURCE_SUBDIRECTORY}\nHeader: include/LogBrew.h\nSource directory: src\nFramework: \
                 Foundation"
            ),
            Self::Cmake => writeln!(
                output,
                "Package: {SDK_PACKAGE_URL}\nRelease tag: {CPP_RELEASE_TAG}\nSource subdirectory: \
                 {CPP_SOURCE_SUBDIRECTORY}\nCore target: {CPP_CORE_TARGET}\nHTTP target: {CPP_HTTP_TARGET} \
                 (optional; requires libcurl)\nDependency:\n{}",
                cmake_dependency_declaration()
            ),
            Self::Python {
                package_manager,
                integration,
            } => {
                let (_, display_name, framework) = integration.details();
                let package_names = python_package_names(integration);
                let requirement = framework
                    .map(|(_, value)| format!("; {value}"))
                    .unwrap_or_default();
                writeln!(
                    output,
                    "Package manager: {package_manager}\nIntegration: {display_name}\nPackages: \
                     {package_names}\nCompatibility review: Python \
                     {PYTHON_MINIMUM_VERSION}{requirement}\nCommand: {}",
                    python_install_command(package_manager, integration)
                )
            }
        }
    }

    /// Returns the safe next action after the plan is displayed.
    const fn next_step(self) -> &'static str {
        match self {
            Self::Swift => SWIFT_NEXT_STEP,
            Self::ObjectiveC => OBJC_NEXT_STEP,
            Self::Cmake => CMAKE_NEXT_STEP,
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
    let detected = detect_projects(root.unwrap_or_else(|| Path::new(".")));
    let install_plan = install_plan(detected.as_slice());
    let next = match install_plan {
        Some(plan) => plan.next_step(),
        None if detected.is_empty() => EMPTY_NEXT_STEP,
        None => SDK_NEXT_STEP,
    };

    if json {
        let body = serde_json::json!({
            "ok": true,
            "auto": auto,
            "yes": yes,
            "install_ready": install_plan.is_some(),
            "install_plan": install_plan.map(InstallPlan::json),
            "detected": detected,
            "next": next,
        });
        return writeln!(output, "{body}");
    }

    writeln!(output, "LogBrew setup plan\nMode: non-mutating plan")?;
    if auto || yes {
        writeln!(output, "Preferences: auto={auto}, yes={yes}")?;
    }
    writeln!(output, "No files changed.")?;
    let readiness = install_plan.map_or("not ready", |_| "ready");
    writeln!(output, "Install: {readiness}")?;
    if let Some(install_plan) = install_plan {
        install_plan.write_human(output)?;
    }
    if detected.is_empty() {
        writeln!(output, "No supported project manifest found.")?;
    } else {
        writeln!(output, "Detected runtimes:")?;
        for detection in &detected {
            writeln!(
                output,
                "- {} ({}) at {}",
                display_runtime(detection.runtime),
                detection.package_manager,
                detection.manifest
            )?;
        }
    }
    writeln!(output, "Next: {next}")
}

/// Builds the copyable `SwiftPM` dependency declaration from canonical fields.
fn swift_dependency_declaration() -> String {
    format!(r#".package(url: "{SDK_PACKAGE_URL}", from: "{SWIFT_MINIMUM_VERSION}")"#)
}

/// Builds a copyable pinned `CMake` `FetchContent` declaration.
fn cmake_dependency_declaration() -> String {
    format!(
        "include(FetchContent)\nFetchContent_Declare(\n  logbrew\n  GIT_REPOSITORY {SDK_PACKAGE_URL}\n  GIT_TAG {CPP_RELEASE_TAG}\n  GIT_SHALLOW TRUE\n  SOURCE_SUBDIR {CPP_SOURCE_SUBDIRECTORY}\n)\nFetchContent_MakeAvailable(logbrew)"
    )
}

/// Returns the space-separated public Python package names.
fn python_package_names(integration: PythonIntegration) -> String {
    std::iter::once("logbrew-sdk")
        .chain(integration.details().2.map(|(name, _)| name))
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

/// Returns the highest-priority released install plan.
fn install_plan(detected: &[ProjectDetection]) -> Option<InstallPlan> {
    detected
        .iter()
        .find_map(|detection| match detection.runtime {
            "objective-c" => Some(InstallPlan::ObjectiveC),
            "swift" | "swift-ios" => Some(InstallPlan::Swift),
            _ => None,
        })
        .or_else(|| {
            detected
                .iter()
                .any(|detection| detection.runtime == "cpp")
                .then_some(InstallPlan::Cmake)
        })
        .or_else(|| {
            detected.iter().find_map(|detection| {
                (detection.runtime == "python").then_some(InstallPlan::Python {
                    package_manager: detection.package_manager,
                    integration: detection
                        .python_integration
                        .unwrap_or(PythonIntegration::Core),
                })
            })
        })
}

/// One detected project manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct ProjectDetection {
    /// Stable runtime key.
    runtime: &'static str,
    /// Package manager or ecosystem used by the runtime.
    package_manager: &'static str,
    /// Released Python integration inferred from bounded local metadata.
    #[serde(skip_serializing)]
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
        left.manifest
            .matches('/')
            .count()
            .cmp(&right.manifest.matches('/').count())
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
    for parent in root.ancestors().skip(1).take(MAX_PARENT_SCAN_DEPTH) {
        collect_manifests(root, parent, MAX_SCAN_DEPTH, detected);
        if !detected.is_empty() {
            return;
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
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
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
                    | "build"
                    | ".git"
                    | ".swiftpm"
                    | ".venv"
                    | "DerivedData"
                    | "node_modules"
                    | "target"
                    | "vendor"
                    | "venv"
            ) || name.starts_with("cmake-build-")
        })
}

/// Maps a manifest path to a runtime and package manager.
fn manifest_runtime(path: &Path) -> Option<(&'static str, &'static str)> {
    let file_name = path.file_name().and_then(std::ffi::OsStr::to_str)?;
    match file_name {
        "Cargo.toml" => Some(("rust", "cargo")),
        "CMakeLists.txt" => Some(("cpp", "cmake")),
        "Package.swift" => Some(("swift", "swift package manager")),
        "composer.json" => Some(("php", "composer")),
        "go.mod" => Some(("go", "go")),
        "package.json" => Some(("node", node_package_manager(path))),
        "Pipfile" => Some(("python", "pipenv")),
        "project.yml" | "project.yaml" => Some((xcodegen_runtime(path), "xcodegen")),
        "pyproject.toml" => Some(("python", python_package_manager(path))),
        _ if file_name.ends_with(".xcodeproj") => Some(("swift-ios", "xcode")),
        _ if file_name.ends_with(".xcworkspace") => Some(("swift-ios", "xcode workspace")),
        _ => None,
    }
}

/// Classifies Objective-C only with complete bounded source evidence.
fn xcodegen_runtime(manifest: &Path) -> &'static str {
    let mut remaining = MAX_APPLE_SOURCE_SCAN_ENTRIES;
    match manifest
        .parent()
        .and_then(|directory| apple_source_languages(directory, 0, &mut remaining))
    {
        Some(1) => "objective-c",
        _ => "swift-ios",
    }
}

/// Returns Objective-C/Swift source bits, or no result when evidence is incomplete.
fn apple_source_languages(directory: &Path, depth: usize, remaining: &mut usize) -> Option<u8> {
    let mut languages = 0;
    for entry in std::fs::read_dir(directory).ok()? {
        *remaining = remaining.checked_sub(1)?;
        let entry = entry.ok()?;
        let path = entry.path();
        let file_type = entry.file_type().ok()?;
        if file_type.is_dir() && !should_skip_dir(path.as_path()) {
            if depth == MAX_SCAN_DEPTH {
                return None;
            }
            languages |= apple_source_languages(path.as_path(), depth + 1, remaining)?;
        } else if file_type.is_file() {
            languages |= match path.extension().and_then(std::ffi::OsStr::to_str) {
                Some("m" | "mm") => 1,
                Some("swift") => 2,
                _ => 0,
            };
        }
        if languages == 3 {
            return Some(languages);
        }
    }
    Some(languages)
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
    if std::fs::symlink_metadata(directory.join("manage.py"))
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
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

/// Returns whether metadata contains a complete normalized distribution token.
fn mentions_python_distribution(text: &str, distribution: &str) -> bool {
    text.split(|character: char| {
        !character.is_ascii_alphanumeric() && character != '-' && character != '_'
    })
    .any(|token| token.eq_ignore_ascii_case(distribution))
}

/// Returns a manifest path relative to the project root.
fn relative_manifest(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| relative_path(root, path).unwrap_or_else(|| display_path(path)),
        display_path,
    )
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

    let mut parts = vec![String::from(".."); root_components.len() - common];
    parts.extend(
        path_components[common..]
            .iter()
            .map(|component| component.as_os_str().to_string_lossy().into_owned()),
    );
    Some(if parts.is_empty() {
        String::from(".")
    } else {
        parts.join("/")
    })
}

/// Returns the source-of-truth preference when several manifests describe one runtime.
fn manifest_priority(path: &str) -> usize {
    match path.rsplit('/').next() {
        Some("project.yml" | "project.yaml") => 0,
        _ if path.ends_with(".xcworkspace") => 1,
        _ if path.ends_with(".xcodeproj") => 2,
        _ => 3,
    }
}

/// Keeps the nearest manifest for each runtime.
fn dedupe_by_runtime(detected: Vec<ProjectDetection>) -> Vec<ProjectDetection> {
    let mut detected = detected;
    let mut runtimes = HashSet::new();
    detected.retain(|detection| runtimes.insert(detection.runtime));
    detected
}

/// Returns human-readable runtime names.
fn display_runtime(runtime: &str) -> &'static str {
    match runtime {
        "go" => "Go",
        "cpp" => "C++",
        "node" => "Node",
        "objective-c" => "Objective-C",
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
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn detects_nearest_manifest_per_runtime() -> TestResult {
        let root = fixture("nearest")?;
        fs::write(root.join("Cargo.toml"), "")?;
        fs::create_dir_all(root.join("crates/logbrew"))?;
        fs::write(root.join("crates/logbrew/Cargo.toml"), "")?;
        fs::write(root.join("package.json"), "{}")?;

        assert_eq!(
            detect_projects(root.as_path()),
            vec![
                detection("node", "npm", "package.json", None),
                detection("rust", "cargo", "Cargo.toml", None),
            ]
        );
        Ok(())
    }

    #[test]
    fn detects_node_package_manager_from_lockfile() -> TestResult {
        for (lockfile, package_manager) in [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lockb", "bun"),
            ("package-lock.json", "npm"),
        ] {
            let root = fixture(package_manager)?;
            fs::write(root.join("package.json"), "{}")?;
            fs::write(root.join(lockfile), "")?;

            assert_detection(&root, "node", package_manager, "package.json", None);
        }
        Ok(())
    }

    #[test]
    fn detects_python_package_manager_from_lockfile() -> TestResult {
        for (manifest, lockfile, package_manager) in [
            ("pyproject.toml", Some("uv.lock"), "uv"),
            ("pyproject.toml", Some("poetry.lock"), "poetry"),
            ("pyproject.toml", Some("Pipfile.lock"), "pipenv"),
            ("Pipfile", None, "pipenv"),
        ] {
            let root = fixture(package_manager)?;
            fs::write(root.join(manifest), "")?;
            if let Some(lockfile) = lockfile {
                fs::write(root.join(lockfile), "")?;
            }

            assert_detection(
                &root,
                "python",
                package_manager,
                manifest,
                Some(PythonIntegration::Core),
            );
        }
        Ok(())
    }

    #[test]
    fn detects_django_from_dynamic_requirements() -> TestResult {
        let root = fixture("django-dynamic-requirements")?;
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"fixture\"\ndynamic = [\"dependencies\"]\n",
        )?;
        fs::write(root.join("requirements.txt"), "Django>=4.2,<6\n")?;

        assert_detection(
            &root,
            "python",
            "pip",
            "pyproject.toml",
            Some(PythonIntegration::Django),
        );
        Ok(())
    }

    #[test]
    fn oversized_python_metadata_falls_back_to_the_core_plan() -> TestResult {
        let root = fixture("oversized-python-metadata")?;
        fs::write(root.join("pyproject.toml"), "Django".repeat(50_000))?;

        assert_detection(
            &root,
            "python",
            "pip",
            "pyproject.toml",
            Some(PythonIntegration::Core),
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn framework_detection_does_not_follow_metadata_symlinks() -> TestResult {
        let root = fixture("python-metadata-symlink")?;
        fs::write(
            root.join("pyproject.toml"),
            "[project]\nname = \"fixture\"\n",
        )?;
        let outside = root.with_extension("outside-requirements");
        fs::write(outside.as_path(), "Django>=5.2\n")?;
        std::os::unix::fs::symlink(outside.as_path(), root.join("requirements.txt"))?;

        assert_detection(
            &root,
            "python",
            "pip",
            "pyproject.toml",
            Some(PythonIntegration::Core),
        );
        fs::remove_file(outside)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn project_detection_rejects_symlinked_manifests() -> TestResult {
        let root = fixture("project-manifest-symlink")?;
        let outside = root.with_extension("outside-pyproject");
        fs::write(
            outside.as_path(),
            "[project]\nname = \"fixture\"\ndependencies = [\"Django\"]\n",
        )?;
        std::os::unix::fs::symlink(outside.as_path(), root.join("pyproject.toml"))?;

        assert!(detect_projects(root.as_path()).is_empty());
        fs::remove_file(outside)?;
        Ok(())
    }

    #[test]
    fn detects_root_cmake_project_without_scanning_build_output() -> TestResult {
        let root = fixture("cmake")?;
        fs::write(
            root.join("CMakeLists.txt"),
            "project(Fixture LANGUAGES CXX)\n",
        )?;
        fs::create_dir_all(root.join("build/nested"))?;
        fs::write(
            root.join("build/nested/CMakeLists.txt"),
            "project(Generated)\n",
        )?;

        assert_detection(&root, "cpp", "cmake", "CMakeLists.txt", None);
        Ok(())
    }

    #[test]
    fn classifies_and_prioritizes_apple_project_manifests() -> TestResult {
        for (name, sources, runtime) in [
            ("objc", &["Sources/main.m"][..], "objective-c"),
            ("swift", &["Sources/App.swift"][..], "swift-ios"),
            (
                "mixed",
                &["Sources/App.m", "Sources/One/Two/Three/Bridge.swift"][..],
                "swift-ios",
            ),
        ] {
            let root = fixture(name)?;
            fs::write(root.join("project.yml"), "name: Checkout\n")?;
            for source in sources {
                let path = root.join(source);
                fs::create_dir_all(path.parent().expect("source fixture has a parent"))?;
                fs::write(path, "// source evidence\n")?;
            }
            assert_detection(&root, runtime, "xcodegen", "project.yml", None);
        }
        for (manifest, package_manager) in [
            ("Checkout.xcodeproj", "xcode"),
            ("Checkout.xcworkspace", "xcode workspace"),
        ] {
            let root = fixture(manifest)?;
            fs::create_dir_all(root.join(manifest))?;

            assert_detection(&root, "swift-ios", package_manager, manifest, None);
        }
        let root = fixture("xcodegen-preference")?;
        fs::write(root.join("project.yaml"), "name: Checkout\n")?;
        fs::create_dir_all(root.join("Checkout.xcodeproj"))?;
        fs::create_dir_all(root.join("Checkout.xcworkspace"))?;

        assert_detection(&root, "swift-ios", "xcodegen", "project.yaml", None);
        Ok(())
    }

    fn detection(
        runtime: &'static str,
        package_manager: &'static str,
        manifest: &str,
        python_integration: Option<PythonIntegration>,
    ) -> ProjectDetection {
        ProjectDetection {
            runtime,
            package_manager,
            python_integration,
            manifest: manifest.to_owned(),
        }
    }

    fn assert_detection(
        root: &std::path::Path,
        runtime: &'static str,
        package_manager: &'static str,
        manifest: &str,
        python_integration: Option<PythonIntegration>,
    ) {
        assert_eq!(
            detect_projects(root),
            vec![detection(
                runtime,
                package_manager,
                manifest,
                python_integration
            )]
        );
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

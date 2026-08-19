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
/// Next step when a public package-registry plan is available.
const PACKAGE_NEXT_STEP: &str =
    "review the compatibility requirements, then add the dependency; no files were changed";
/// Current released Java SDK coordinate and compatibility floor.
const JAVA_SDK_VERSION: &str = "0.1.4";
const JAVA_MINIMUM_VERSION: &str = ">=11";
/// Minimum Python version required by the current public Python SDK family.
const PYTHON_MINIMUM_VERSION: &str = ">=3.10";
/// Supported Django range for the current public Django integration.
const DJANGO_VERSION_REQUIREMENT: &str = "Django>=4.2.30,<6";
/// Minimum Flask version required by the current public Flask integration.
const FLASK_MINIMUM_VERSION: &str = "Flask>=3.1";
/// Minimum `FastAPI` version required by the current public `FastAPI` integration.
const FASTAPI_MINIMUM_VERSION: &str = "FastAPI>=0.111.1";
/// Minimum PHP version required by the current public PHP SDK.
const PHP_MINIMUM_VERSION: &str = "^8.2";
/// Supported Symfony range for the current public integration.
const SYMFONY_VERSION_REQUIREMENT: &str = "Symfony^6.4 || ^7.0 || ^8.0";
/// Minimum Ruby version required by the current public Ruby SDK.
const RUBY_MINIMUM_VERSION: &str = ">=2.6";
/// Copyable Bundler command pinned to the current public Ruby SDK family.
const RUBY_INSTALL_COMMAND: &str = "bundle add logbrew-sdk --version \"~> 0.1.5\"";
const SVELTE_PACKAGES: &str = "@logbrew/sdk @logbrew/browser @logbrew/svelte";
const REACT_EXPRESS_PACKAGES: &str =
    "@logbrew/sdk @logbrew/browser @logbrew/react @logbrew/node @logbrew/express";
/// Maximum bytes read from a manifest while detecting a framework.
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
                               pyproject.toml, Pipfile, pom.xml, build.gradle, build.gradle.kts, \
                               Cargo.toml, Package.swift, project.yml, project.yaml, .xcodeproj, \
                               .xcworkspace, CMakeLists.txt, go.mod, composer.json, or Gemfile.";

/// Stable key, display name, and optional framework package requirement.
type PythonIntegration = (
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
);
const PYTHON_CORE: PythonIntegration = ("python", "Python", None);
const PYTHON_DJANGO: PythonIntegration = (
    "django",
    "Django",
    Some(("logbrew-django", DJANGO_VERSION_REQUIREMENT)),
);
const PYTHON_FLASK: PythonIntegration = (
    "flask",
    "Flask",
    Some(("logbrew-flask", FLASK_MINIMUM_VERSION)),
);
const PYTHON_FASTAPI: PythonIntegration = (
    "fastapi",
    "FastAPI",
    Some(("logbrew-fastapi", FASTAPI_MINIMUM_VERSION)),
);
type PythonSetup = (PythonIntegration, bool);

/// Released package-registry integration selected from local project evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageIntegration {
    Java(bool),
    Python(PythonSetup),
    Php(bool),
    Ruby(bool),
    SvelteKit,
    ReactExpress,
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
    /// Package-registry plan.
    Package {
        /// Detected package manager.
        package_manager: &'static str,
        /// Detected language/framework integration.
        integration: PackageIntegration,
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
            Self::Package {
                package_manager,
                integration,
            } => match integration {
                PackageIntegration::Python(value) => python_plan_json(package_manager, value),
                PackageIntegration::Java(value) => java_plan_json(package_manager, value),
                PackageIntegration::Php(value) => composer_plan_json(package_manager, value),
                PackageIntegration::Ruby(value) => ruby_plan_json(package_manager, value),
                PackageIntegration::SvelteKit => svelte_plan_json(package_manager),
                PackageIntegration::ReactExpress => react_express_plan_json(package_manager),
            },
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
            Self::Package {
                package_manager,
                integration,
            } => write_package_human(package_manager, integration, output),
        }
    }

    /// Returns the safe next action after the plan is displayed.
    const fn next_step(self) -> &'static str {
        match self {
            Self::Swift => SWIFT_NEXT_STEP,
            Self::ObjectiveC => OBJC_NEXT_STEP,
            Self::Cmake => CMAKE_NEXT_STEP,
            Self::Package { .. } => PACKAGE_NEXT_STEP,
        }
    }
}

fn package_next_action() -> serde_json::Value {
    serde_json::json!({
        "code": "review_compatibility_and_install",
        "target": "project_environment",
    })
}

fn java_plan_json(package_manager: &str, spring: bool) -> serde_json::Value {
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": package_manager,
        "package_manager": package_manager,
        "integration": if spring { "spring" } else { "java" },
        "package": {
            "group_id": "co.logbrew",
            "artifact_id": "logbrew-sdk",
            "version": JAVA_SDK_VERSION,
        },
        "compatibility": {
            "status": "review_required",
            "requires_java": JAVA_MINIMUM_VERSION,
            "requires_framework": spring.then_some("Spring Boot 3+"),
        },
        "dependency_declaration": java_dependency_declaration(package_manager),
        "next_action": package_next_action(),
    })
}

fn python_plan_json(package_manager: &str, setup: PythonSetup) -> serde_json::Value {
    let (integration, celery) = setup;
    let (key, _, framework) = integration;
    let mut core = npm_package("logbrew-sdk", "core");
    if celery {
        core["extras"] = serde_json::json!(["celery"]);
    }
    let packages = std::iter::once(core)
        .chain(framework.map(|(name, _)| npm_package(name, "framework")))
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
        "install_command": python_install_command(package_manager, setup),
        "next_action": package_next_action(),
    })
}

fn composer_plan_json(package_manager: &str, symfony: bool) -> serde_json::Value {
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": "composer",
        "package_manager": package_manager,
        "integration": if symfony { "symfony" } else { "php" },
        "package": "logbrew/sdk",
        "framework_manifest": symfony.then_some("config/bundles.php"),
        "compatibility": {
            "status": "review_required",
            "requires_php": PHP_MINIMUM_VERSION,
            "requires_framework": symfony.then_some(SYMFONY_VERSION_REQUIREMENT),
        },
        "install_command": "composer require logbrew/sdk",
        "next_action": package_next_action(),
    })
}

fn ruby_plan_json(package_manager: &str, rails: bool) -> serde_json::Value {
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": "rubygems",
        "package_manager": package_manager,
        "integration": if rails { "rails" } else { "ruby" },
        "package": "logbrew-sdk",
        "framework_manifest": rails.then_some("config/application.rb"),
        "compatibility": {
            "status": "review_required",
            "requires_ruby": RUBY_MINIMUM_VERSION,
            "requires_framework": serde_json::Value::Null,
        },
        "install_command": RUBY_INSTALL_COMMAND,
        "next_action": package_next_action(),
    })
}

fn svelte_plan_json(package_manager: &str) -> serde_json::Value {
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": "npm",
        "package_manager": package_manager,
        "integration": "sveltekit",
        "packages": [
            npm_package("@logbrew/sdk", "core"),
            npm_package("@logbrew/browser", "delivery"),
            npm_package("@logbrew/svelte", "framework"),
        ],
        "compatibility": {
            "status": "review_required",
            "requires_node": ">=18",
            "requires_framework": "Svelte >=5",
        },
        "surfaces": [
            {"surface": "browser", "integration": "svelte", "credential_kind": "browser", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
            {"surface": "server", "integration": "sveltekit", "credential_kind": "server", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
        ],
        "install_command": javascript_install_command(package_manager, SVELTE_PACKAGES),
        "next_action": package_next_action(),
    })
}

fn react_express_plan_json(package_manager: &str) -> serde_json::Value {
    serde_json::json!({
        "mode": "non_mutating",
        "ecosystem": "npm",
        "package_manager": package_manager,
        "integration": "react_express",
        "packages": REACT_EXPRESS_PACKAGES.split_whitespace().collect::<Vec<_>>(),
        "compatibility": {
            "status": "review_required",
            "requires_node": ">=18",
            "requires_frameworks": ["React >=18", "Express >=4"],
        },
        "surfaces": [
            {"surface": "browser", "integration": "react", "credential_kind": "browser", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
            {"surface": "server", "integration": "express", "credential_kind": "server", "service_name_required": true, "deployment_context_required": ["environment", "release"]},
        ],
        "install_command": javascript_install_command(package_manager, REACT_EXPRESS_PACKAGES),
        "next_action": package_next_action(),
    })
}

fn npm_package(name: &str, role: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "role": role,
        "version_requirement": {"kind": "latest_compatible"},
    })
}

fn write_package_human<W: std::io::Write>(
    package_manager: &str,
    integration: PackageIntegration,
    output: &mut W,
) -> Result<(), std::io::Error> {
    match integration {
        PackageIntegration::Java(spring) => writeln!(
            output,
            "Package manager: {package_manager}\nIntegration: {}\nPackage: co.logbrew:logbrew-sdk:{JAVA_SDK_VERSION}\nCompatibility review: Java {JAVA_MINIMUM_VERSION}{}\nDependency: {}",
            if spring { "Spring" } else { "Java" },
            if spring { "; Spring Boot 3+" } else { "" },
            java_dependency_declaration(package_manager),
        ),
        PackageIntegration::Python(setup @ (integration, _)) => {
            let (_, display, framework) = integration;
            let requirement = framework.map_or(String::new(), |(_, value)| format!("; {value}"));
            writeln!(
                output,
                "Package manager: {package_manager}\nIntegration: {display}\nPackages: {}\nCompatibility review: Python {PYTHON_MINIMUM_VERSION}{requirement}\nCommand: {}",
                python_package_names(setup),
                python_install_command(package_manager, setup),
            )
        }
        PackageIntegration::Php(symfony) => writeln!(
            output,
            "Package manager: {package_manager}\nIntegration: {}\nPackage: logbrew/sdk\nCompatibility review: PHP {PHP_MINIMUM_VERSION}{}\nCommand: composer require logbrew/sdk",
            if symfony { "Symfony" } else { "PHP" },
            if symfony {
                "; Symfony^6.4 || ^7.0 || ^8.0"
            } else {
                ""
            },
        ),
        PackageIntegration::Ruby(rails) => writeln!(
            output,
            "Package manager: {package_manager}\nIntegration: {}\nPackage: logbrew-sdk\nCompatibility review: Ruby {RUBY_MINIMUM_VERSION}\nCommand: {RUBY_INSTALL_COMMAND}",
            if rails { "Rails" } else { "Ruby" },
        ),
        PackageIntegration::SvelteKit => writeln!(
            output,
            "Package manager: {package_manager}\nIntegration: SvelteKit\nPackages: {SVELTE_PACKAGES}\nCompatibility review: Node >=18; Svelte >=5\nSurfaces:\n- browser: Svelte; key kind: browser; stable service name, environment, and release required\n- server: SvelteKit; key kind: server; stable service name, environment, and release required\nCommand: {}",
            javascript_install_command(package_manager, SVELTE_PACKAGES),
        ),
        PackageIntegration::ReactExpress => writeln!(
            output,
            "Package manager: {package_manager}\nIntegration: React + Express\nPackages: {REACT_EXPRESS_PACKAGES}\nCompatibility review: Node >=18; React >=18; Express >=4\nSurfaces:\n- browser: React; key kind: browser; stable service name, environment, and release required\n- server: Express; key kind: server; stable service name, environment, and release required\nCommand: {}",
            javascript_install_command(package_manager, REACT_EXPRESS_PACKAGES),
        ),
    }
}

fn javascript_install_command(package_manager: &str, packages: &str) -> String {
    match package_manager {
        "pnpm" | "yarn" | "bun" => format!("{package_manager} add {packages}"),
        _ => format!("npm install {packages}"),
    }
}

fn java_dependency_declaration(package_manager: &str) -> String {
    match package_manager {
        "maven" => format!(
            "<dependency>\n  <groupId>co.logbrew</groupId>\n  <artifactId>logbrew-sdk</artifactId>\n  <version>{JAVA_SDK_VERSION}</version>\n</dependency>"
        ),
        "gradle-kotlin" => format!("implementation(\"co.logbrew:logbrew-sdk:{JAVA_SDK_VERSION}\")"),
        _ => format!("implementation 'co.logbrew:logbrew-sdk:{JAVA_SDK_VERSION}'"),
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
fn python_package_names((integration, celery): PythonSetup) -> String {
    let core = if celery {
        "\"logbrew-sdk[celery]\""
    } else {
        "logbrew-sdk"
    };
    match integration.2 {
        Some((name, _)) => format!("{core} {name}"),
        None => core.to_owned(),
    }
}

/// Builds a copyable package-manager command from public package names.
fn python_install_command(package_manager: &str, setup: PythonSetup) -> String {
    let packages = python_package_names(setup);
    match package_manager {
        "uv" | "poetry" => format!("{package_manager} add {packages}"),
        "pipenv" => format!("pipenv install {packages}"),
        _ => format!("python3 -m pip install --upgrade {packages}"),
    }
}

/// Returns the highest-priority released install plan.
fn install_plan(detected: &[ProjectDetection]) -> Option<InstallPlan> {
    detected
        .iter()
        .filter_map(|detection| {
            let (priority, plan) = match detection.runtime {
                "objective-c" => (0, InstallPlan::ObjectiveC),
                "swift" | "swift-ios" => (1, InstallPlan::Swift),
                "cpp" => (2, InstallPlan::Cmake),
                "java" | "python" | "php" | "ruby" | "sveltekit" | "react-express" => (
                    match detection.runtime {
                        "java" => 3,
                        "python" => 4,
                        "php" => 5,
                        "ruby" => 6,
                        _ => 7,
                    },
                    InstallPlan::Package {
                        package_manager: detection.package_manager,
                        integration: detection.package_integration?,
                    },
                ),
                _ => return None,
            };
            Some((priority, plan))
        })
        .min_by_key(|(priority, _)| *priority)
        .map(|(_, plan)| plan)
}

/// One detected project manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct ProjectDetection {
    /// Stable runtime key.
    runtime: &'static str,
    /// Package manager or ecosystem used by the runtime.
    package_manager: &'static str,
    /// Released package integration inferred from bounded local metadata.
    #[serde(skip_serializing)]
    package_integration: Option<PackageIntegration>,
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
    detected.sort_by(|left, right| detection_key(left).cmp(&detection_key(right)));
    let mut runtimes = HashSet::new();
    detected.retain(|detection| runtimes.insert(detection.runtime));
    detected
}

fn detection_key(detection: &ProjectDetection) -> (usize, &'static str, usize, &str) {
    (
        detection.manifest.matches('/').count(),
        detection.runtime,
        match detection.manifest.rsplit('/').next() {
            Some("project.yml" | "project.yaml") => 0,
            _ if detection.manifest.ends_with(".xcworkspace") => 1,
            _ if detection.manifest.ends_with(".xcodeproj") => 2,
            _ => 3,
        },
        &detection.manifest,
    )
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
    let package_integration = match runtime {
        "java" => Some(PackageIntegration::Java(
            read_framework_manifest(path).is_some_and(|text| {
                ["org.springframework", "spring-boot", "spring-kafka"]
                    .iter()
                    .any(|marker| text.contains(marker))
            }),
        )),
        "node" => detect_javascript_integration(path),
        "python" => Some(PackageIntegration::Python(detect_python_integration(path))),
        "php" => Some(PackageIntegration::Php(has_project_file(
            path,
            "config/bundles.php",
        ))),
        "ruby" => Some(PackageIntegration::Ruby(has_project_file(
            path,
            "config/application.rb",
        ))),
        _ => None,
    };
    Some(ProjectDetection {
        runtime: match package_integration {
            Some(PackageIntegration::SvelteKit) => "sveltekit",
            Some(PackageIntegration::ReactExpress) => "react-express",
            _ => runtime,
        },
        package_manager,
        package_integration,
        manifest: relative_manifest(root, path),
    })
}

/// Detects one released JavaScript integration from bounded standard dependency maps.
fn detect_javascript_integration(manifest: &Path) -> Option<PackageIntegration> {
    let text = read_framework_manifest(manifest)?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    let has = |package| {
        [
            "dependencies",
            "devDependencies",
            "optionalDependencies",
            "peerDependencies",
        ]
        .into_iter()
        .filter_map(|field| value.get(field)?.as_object())
        .any(|dependencies| dependencies.contains_key(package))
    };
    if has("@sveltejs/kit") {
        Some(PackageIntegration::SvelteKit)
    } else {
        (!has("next") && !has("react-native") && has("react") && has("express"))
            .then_some(PackageIntegration::ReactExpress)
    }
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
        "build.gradle" => Some(("java", "gradle")),
        "build.gradle.kts" => Some(("java", "gradle-kotlin")),
        "Package.swift" => Some(("swift", "swift package manager")),
        "composer.json" => Some(("php", "composer")),
        "Gemfile" => Some(("ruby", "bundler")),
        "go.mod" => Some(("go", "go")),
        "package.json" => Some(("node", node_package_manager(path))),
        "pom.xml" => Some(("java", "maven")),
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
fn detect_python_integration(manifest: &Path) -> PythonSetup {
    let Some(directory) = manifest.parent() else {
        return (PYTHON_CORE, false);
    };
    let text = std::iter::once(manifest.to_path_buf())
        .chain(
            [
                "requirements.txt",
                "requirements.in",
                "setup.cfg",
                "setup.py",
            ]
            .map(|file| directory.join(file)),
        )
        .filter_map(|path| read_framework_manifest(&path))
        .collect::<Vec<_>>()
        .join("\n");
    let integration = if has_project_file(manifest, "manage.py") {
        PYTHON_DJANGO
    } else {
        [
            ("django", PYTHON_DJANGO),
            ("fastapi", PYTHON_FASTAPI),
            ("flask", PYTHON_FLASK),
        ]
        .into_iter()
        .find_map(|(name, integration)| {
            mentions_python_distribution(&text, name).then_some(integration)
        })
        .unwrap_or(PYTHON_CORE)
    };
    (integration, mentions_python_distribution(&text, "celery"))
}

/// Checks framework evidence without following a metadata symlink.
fn has_project_file(manifest: &Path, relative: &str) -> bool {
    std::fs::symlink_metadata(manifest.with_file_name(relative))
        .is_ok_and(|metadata| metadata.file_type().is_file())
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

    let relative = std::iter::repeat_n("..".to_owned(), root_components.len() - common)
        .chain(
            path_components[common..]
                .iter()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join("/");
    Some(if relative.is_empty() {
        String::from(".")
    } else {
        relative
    })
}

/// Returns human-readable runtime names.
fn display_runtime(runtime: &str) -> &'static str {
    match runtime {
        "go" => "Go",
        "java" => "Java",
        "cpp" => "C++",
        "node" => "Node",
        "objective-c" => "Objective-C",
        "php" => "PHP",
        "python" => "Python",
        "react-express" => "React + Express",
        "ruby" => "Ruby",
        "rust" => "Rust",
        "sveltekit" => "SvelteKit",
        "swift" => "Swift",
        "swift-ios" => "Swift/iOS",
        _ => "Project",
    }
}

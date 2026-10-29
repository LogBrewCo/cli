//! Applies setup: sign-in, project, SDK, ingest key, init code, agent config, and a test event.

use super::recipe::{self, Action, FileChange, KEY_VARIABLE};
use crate::{CliEnvironment, RuntimeError};
use std::io::{BufRead, Write};
use std::path::Path;

/// Shown in diffs and plans where the new ingest key goes; the key itself is never printed.
const KEY_PLACEHOLDER: &str = "<new ingest key>";
/// Shown where the id of a project setup creates goes.
const PROJECT_PLACEHOLDER: &str = "<new project id>";
/// Test-event attempts; the server reads each event back before it answers.
const VERIFY_ATTEMPTS: u32 = 2;
/// Largest first-run response setup reads.
const FIRST_RUN_RESPONSE_LIMIT: usize = 16 * 1024;

/// Choices for one setup run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WizardOptions {
    /// Apply without prompts.
    pub yes: bool,
    /// Print one JSON report instead of human output.
    pub json: bool,
    /// Optional additions to the project.
    pub extras: WizardExtras,
    /// Existing project to set up.
    pub project_id: Option<String>,
}

/// Optional additions setup makes only when asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WizardExtras {
    /// Add the hosted MCP server to detected agent clients.
    pub mcp: bool,
    /// Load removable sample data into a project that has no events yet.
    pub sample_data: bool,
}

/// The project setup sends events to.
enum ProjectChoice {
    /// An existing project: id and name.
    Existing(String, String),
    /// A project setup creates with this name.
    New(String),
}

/// Collects human lines and the JSON report.
struct Report<'a, W: Write> {
    /// Destination.
    output: &'a mut W,
    /// Whether to print JSON at the end instead of human lines.
    json: bool,
    /// Completed steps.
    steps: Vec<serde_json::Value>,
}

impl<W: Write> Report<'_, W> {
    /// Prints one human line.
    fn say(&mut self, line: &str) -> Result<(), RuntimeError> {
        if !self.json {
            writeln!(self.output, "{line}")?;
        }
        Ok(())
    }

    /// Records one step and prints its human line.
    fn step(&mut self, value: serde_json::Value, line: &str) -> Result<(), RuntimeError> {
        self.steps.push(value);
        self.say(line)
    }
}

/// The detected project and the env file setup keeps its key in.
struct Project {
    /// Project directory: the folder holding the manifest.
    dir: std::path::PathBuf,
    /// Detected install target.
    target: super::SetupTarget,
    /// Integration or ecosystem key.
    integration: String,
    /// Project and service name derived from the directory.
    service: String,
    /// Env file relative to `dir`.
    env_file: &'static str,
    /// Current env file text.
    env_text: String,
}

impl Project {
    /// Detects the project in the directory setup runs in.
    fn detect(env: &CliEnvironment) -> Result<Self, RuntimeError> {
        let root = env.cwd.clone().ok_or_else(no_project)?;
        let target = super::setup_target(root.as_path()).ok_or_else(no_project)?;
        let dir = root
            .join(target.manifest.as_str())
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(no_project)?;
        let integration = recipe::integration(&target.plan).to_owned();
        let env_file = recipe::env_file(integration.as_str());
        let env_text = match recipe::read(dir.as_path(), env_file) {
            Some(text) => text,
            None if std::fs::symlink_metadata(dir.join(env_file)).is_ok() => {
                return Err(unsafe_file());
            }
            None => String::new(),
        };
        Ok(Self {
            service: service_name(dir.as_path()),
            dir,
            target,
            integration,
            env_file,
            env_text,
        })
    }

    /// Returns the ingest key already in the env file.
    fn existing_key(&self) -> Option<String> {
        env_value(self.env_text.as_str(), KEY_VARIABLE)
    }
}

/// Planned changes, shown before anything is applied.
struct Plan {
    /// File changes in write order.
    changes: Vec<FileChange>,
    /// SDK install step.
    install: Action,
    /// Init code step.
    init: Action,
    /// MCP step JSON.
    mcp: serde_json::Value,
}

/// Runs setup in the current directory.
///
/// With `yes`, nothing is asked. Otherwise each question is read from `input`,
/// and every file change is shown as a diff before anything is applied.
///
/// # Errors
///
/// Returns [`RuntimeError`] when no supported project is found, sign-in or an
/// API call fails, a file changed while setup ran, or the test event did not arrive.
pub async fn run_wizard<R: BufRead, W: Write>(
    env: &CliEnvironment,
    options: &WizardOptions,
    input: &mut R,
    output: &mut W,
) -> Result<(), RuntimeError> {
    let project = Project::detect(env)?;
    let mut report = Report {
        output,
        json: options.json,
        steps: Vec::new(),
    };
    report.say(&format!(
        "LogBrew setup\nFramework: {} ({}) from {}",
        super::display_runtime(project.target.runtime),
        project.target.plan["package_manager"]
            .as_str()
            .unwrap_or("source"),
        project.target.manifest
    ))?;
    sign_in(env, &mut report).await?;
    let choice = choose_project(env, options, &project, &mut report, input).await?;
    let plan = plan(env, options, &project, &choice, &mut report, input)?;
    if !options.yes && !confirm(&mut report, input, "Apply these changes? [Y/n] ", true)? {
        return report.say("No changes made.");
    }
    for change in &plan.changes {
        drop(safe_path(project.dir.as_path(), change.path.as_str())?);
    }
    let (project_id, key) = credentials(env, &project, choice, &mut report).await?;
    let files = write_changes(
        &project,
        &plan,
        project_id.as_str(),
        key.as_deref(),
        &mut report,
    )?;
    let installed = install(&project, &plan, &mut report).await?;
    let mcp = plan.mcp["status"]
        .as_str()
        .unwrap_or_default()
        .replace('_', " ");
    report.step(plan.mcp, &format!("MCP server: {mcp}"))?;
    if options.extras.sample_data {
        let step = sample_data(env, project_id.as_str()).await?;
        let counts = &step["counts"];
        let line = match step["status"].as_str() {
            Some("loaded") => format!(
                "Sample data: loaded (issues {}, logs {}, trace spans {}, actions {})",
                counts["issue_events"], counts["logs"], counts["trace_spans"], counts["actions"]
            ),
            Some("skipped") => String::from("Sample data: skipped; the project already has events"),
            _ => String::from("Sample data: failed to load"),
        };
        report.step(step, line.as_str())?;
    }
    report.say("Test event: sending…")?;
    let send_key = key.or_else(|| project.existing_key()).unwrap_or_default();
    let verified = verify(env, project_id.as_str(), send_key.as_str()).await?;
    let arrived = verified.is_some();
    let line = verified.as_ref().map_or_else(
        || String::from("Test event: did not arrive"),
        |step| {
            format!(
                "Test event: arrived in LogBrew in {} ms\nRead it back: logbrew read logs --project {project_id} --trace {}",
                step["elapsed_ms"], step["trace_id"].as_str().unwrap_or_default()
            )
        },
    );
    report.step(
        verified.unwrap_or_else(|| serde_json::json!({"step": "verify", "status": "not_arrived"})),
        line.as_str(),
    )?;
    finish(&project, &mut report, &files, installed, arrived)
}

/// Signs in through the browser when no credential is stored.
async fn sign_in<W: Write>(
    env: &CliEnvironment,
    report: &mut Report<'_, W>,
) -> Result<(), RuntimeError> {
    let signed_in = crate::auth::inspect_auth_snapshot(env)?.authenticated;
    if !signed_in {
        report.say("Sign-in: opening your browser…")?;
        crate::auth::execute_login(
            env,
            crate::LoginProvider::default(),
            true,
            false,
            &mut Vec::new(),
        )
        .await?;
    }
    report.step(
        serde_json::json!({"step": "sign_in", "status": if signed_in { "kept" } else { "done" }}),
        if signed_in {
            "Sign-in: already signed in"
        } else {
            "Sign-in: done"
        },
    )
}

/// Picks the project from `--project`, the env file, a prompt, or the directory name.
async fn choose_project<R: BufRead, W: Write>(
    env: &CliEnvironment,
    options: &WizardOptions,
    project: &Project,
    report: &mut Report<'_, W>,
    input: &mut R,
) -> Result<ProjectChoice, RuntimeError> {
    let projects = crate::projects::list(env).await?;
    let key_present = project.existing_key().is_some();
    let existing =
        |(id, name): &(String, String)| ProjectChoice::Existing(id.clone(), name.clone());
    let known_id = options.project_id.clone().or_else(|| {
        env_value(project.env_text.as_str(), "LOGBREW_PROJECT_ID")
            .filter(|id| crate::ids::is_uuid(id))
    });
    let choice = match known_id {
        Some(id) => projects
            .iter()
            .find(|(project, _)| *project == id)
            .map(existing)
            .ok_or(RuntimeError::Unavailable {
                message: "the project was not found in your account",
                next: "run logbrew projects, then rerun setup with --project <project_id>",
            })?,
        None if options.yes => projects
            .iter()
            .find(|(_, name)| *name == project.service)
            .map(existing)
            .or_else(|| (!key_present).then(|| ProjectChoice::New(project.service.clone())))
            .ok_or(RuntimeError::Unavailable {
                message: "the env file already has an ingest key for an unknown project",
                next: "rerun setup with --project <project_id>",
            })?,
        None => ask_project(
            report,
            input,
            &projects,
            project.service.as_str(),
            key_present,
        )?,
    };
    let line = match &choice {
        ProjectChoice::Existing(_, name) => format!("Project: {name}"),
        ProjectChoice::New(name) => format!("Project: {name} (new)"),
    };
    report.say(&line)?;
    Ok(choice)
}

/// Plans every change and prints the diffs.
fn plan<R: BufRead, W: Write>(
    env: &CliEnvironment,
    options: &WizardOptions,
    project: &Project,
    choice: &ProjectChoice,
    report: &mut Report<'_, W>,
    input: &mut R,
) -> Result<Plan, RuntimeError> {
    let dir = project.dir.as_path();
    let project_id = match choice {
        ProjectChoice::Existing(id, _) => id.as_str(),
        ProjectChoice::New(_) => PROJECT_PLACEHOLDER,
    };
    let mut changes = Vec::new();
    changes.extend(env_change(project, project_id));
    changes.extend(gitignore_change(dir, project.env_file));
    let manifest = project
        .target
        .manifest
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let install = recipe::install(dir, manifest, &project.target.plan);
    let init = recipe::init(dir, &project.target.plan, project.service.as_str());
    changes.extend(
        [&install, &init]
            .into_iter()
            .filter_map(Action::edit)
            .cloned(),
    );
    let clients = recipe::mcp(dir, env.home.as_deref());
    let edits = clients
        .iter()
        .filter_map(|(client, action)| Some((client.name, action.edit()?)))
        .collect::<Vec<_>>();
    let names = edits
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(" & ");
    let add_mcp = !edits.is_empty()
        && (options.extras.mcp
            || (!options.yes
                && confirm(
                    report,
                    input,
                    &format!("Add the LogBrew MCP server to {names} for this project? [y/N] "),
                    false,
                )?));
    if add_mcp {
        changes.extend(edits.iter().map(|(_, change)| (*change).clone()));
    }
    let status = match (clients.is_empty(), edits.is_empty(), add_mcp) {
        (true, _, _) => "no_clients",
        (false, true, _) => "present",
        (false, false, true) => "done",
        (false, false, false) => "skipped",
    };
    if !changes.is_empty() {
        report.say("Changes:")?;
    }
    for change in &changes {
        report.say(diff(change).trim_end())?;
    }
    if let Action::Run(argv) = &install {
        report.say(&format!("Command: {}", argv.join(" ")))?;
    }
    Ok(Plan {
        changes,
        install,
        init,
        mcp: serde_json::json!({
            "step": "mcp",
            "status": status,
            "clients": clients
                .iter()
                .map(|(client, _)| serde_json::json!({"name": client.name, "config": client.path}))
                .collect::<Vec<_>>(),
        }),
    })
}

/// Creates the project or ingest key setup needs and returns the project id and new key.
async fn credentials<W: Write>(
    env: &CliEnvironment,
    project: &Project,
    choice: ProjectChoice,
    report: &mut Report<'_, W>,
) -> Result<(String, Option<String>), RuntimeError> {
    let (id, name, key, created) = match choice {
        ProjectChoice::New(name) => {
            let options = crate::ProjectCreateOptions {
                name: name.clone(),
                runtime: Some(project.target.runtime.to_owned()),
                environment: Some(String::from("development")),
                repository: None,
                ingest_key_file: String::new(),
                abandon_retry: false,
            };
            let (id, key) = crate::project_create::create_project_for_setup(env, &options).await?;
            (id, name, Some(key), true)
        }
        ProjectChoice::Existing(id, name) if project.existing_key().is_none() => {
            let options = crate::ProjectIngestKeyCreateOptions {
                project_id: id.clone(),
                label: String::from("logbrew setup"),
                kind: String::from("server"),
                ingest_key_file: String::new(),
                abandon_retry: false,
            };
            let key = crate::project_create::create_ingest_key_for_setup(env, &options).await?;
            (id, name, Some(key), false)
        }
        ProjectChoice::Existing(id, name) => (id, name, None, false),
    };
    report.step(
        serde_json::json!({
            "step": "project",
            "status": if created { "created" } else { "kept" },
            "project": {"id": id, "name": name},
            "env_file": project.env_file,
            "key_variable": KEY_VARIABLE,
            "key": if key.is_some() { "created" } else { "kept" },
        }),
        &format!(
            "Ingest key: {} in {} as {KEY_VARIABLE}",
            if key.is_some() {
                "created & stored"
            } else {
                "kept"
            },
            project.env_file
        ),
    )?;
    Ok((id, key))
}

/// Writes every planned file change and returns the JSON file list.
fn write_changes<W: Write>(
    project: &Project,
    plan: &Plan,
    project_id: &str,
    key: Option<&str>,
    report: &mut Report<'_, W>,
) -> Result<Vec<serde_json::Value>, RuntimeError> {
    let replace = |text: &str| {
        let text = text.replace(PROJECT_PLACEHOLDER, project_id);
        key.map_or_else(|| text.clone(), |key| text.replace(KEY_PLACEHOLDER, key))
    };
    let mut files = Vec::new();
    for change in &plan.changes {
        write_change(project.dir.as_path(), change, &replace)?;
        files.push(serde_json::json!({
            "path": change.path,
            "action": if change.before.is_some() { "update" } else { "create" },
            "diff": diff(change),
        }));
        report.say(&format!("Wrote {}", change.path))?;
    }
    Ok(files)
}

/// Runs the install command and records the install and init steps.
async fn install<W: Write>(
    project: &Project,
    plan: &Plan,
    report: &mut Report<'_, W>,
) -> Result<bool, RuntimeError> {
    let installed = if let Action::Run(argv) = &plan.install {
        let ok = run(project.dir.as_path(), argv).await;
        let status = if ok { "done" } else { "failed" };
        report.step(
            serde_json::json!({"step": "install", "status": status, "command": argv.join(" ")}),
            &format!("Install: {status} ({})", argv.join(" ")),
        )?;
        ok
    } else {
        report.step(
            step_json("install", &plan.install),
            &step_line("Install", &plan.install),
        )?;
        true
    };
    report.step(
        step_json("init", &plan.init),
        &step_line("Init code", &plan.init),
    )?;
    Ok(installed)
}

/// Prints the result and fails when the install or test event did not succeed.
fn finish<W: Write>(
    project: &Project,
    report: &mut Report<'_, W>,
    files: &[serde_json::Value],
    installed: bool,
    arrived: bool,
) -> Result<(), RuntimeError> {
    let next = if !installed {
        "fix the install error above, then rerun logbrew setup"
    } else if !arrived {
        "check the key in the env file, then rerun logbrew setup"
    } else if recipe::loads_env_file(project.integration.as_str()) {
        "start your app; its errors & logs now go to LogBrew"
    } else {
        "load the env file into your app's environment, then start it"
    };
    if report.json {
        let body = serde_json::json!({
            "ok": installed && arrived,
            "framework": {
                "runtime": project.target.runtime,
                "integration": project.integration,
                "manifest": project.target.manifest,
            },
            "steps": report.steps,
            "files": files,
            "next": next,
        });
        writeln!(report.output, "{body}")?;
    } else {
        writeln!(report.output, "Next: {next}")?;
    }
    if installed && arrived {
        Ok(())
    } else {
        Err(RuntimeError::Unavailable {
            message: "setup did not finish",
            next,
        })
    }
}

/// Builds the JSON for a step that needs no command.
fn step_json(step: &str, action: &Action) -> serde_json::Value {
    let (status, instructions) = match action {
        Action::Present => ("present", None),
        Action::Edit(_) | Action::Run(_) => ("done", None),
        Action::Manual(text) => ("manual", Some(text.as_str())),
        Action::NotNeeded => ("not_needed", None),
    };
    let file = action.edit().map(|change| change.path.as_str());
    serde_json::json!({"step": step, "status": status, "file": file, "instructions": instructions})
}

/// Builds the human line for a step that needs no command.
fn step_line(label: &str, action: &Action) -> String {
    match action {
        Action::Present => format!("{label}: already in place"),
        Action::Edit(change) => format!("{label}: added to {}", change.path),
        Action::Run(argv) => format!("{label}: {}", argv.join(" ")),
        Action::Manual(text) => format!("{label}: add this by hand\n{text}"),
        Action::NotNeeded => format!("{label}: not needed"),
    }
}

/// Asks which project to use.
fn ask_project<R: BufRead, W: Write>(
    report: &mut Report<'_, W>,
    input: &mut R,
    projects: &[(String, String)],
    service: &str,
    key_present: bool,
) -> Result<ProjectChoice, RuntimeError> {
    let default = projects
        .iter()
        .position(|(_, name)| name == service)
        .map_or_else(|| String::from("n"), |index| (index + 1).to_string());
    report.say("Projects:")?;
    for (index, (_, name)) in projects.iter().enumerate() {
        report.say(&format!("  {}. {name}", index + 1))?;
    }
    if !key_present {
        report.say(&format!("  n. Create \"{service}\""))?;
    }
    for _attempt in 0..3 {
        let answer = ask(report, input, &format!("Project [Enter for {default}]: "))?;
        let answer = if answer.is_empty() {
            default.clone()
        } else {
            answer
        };
        if answer == "n" && !key_present {
            return Ok(ProjectChoice::New(service.to_owned()));
        }
        if let Some((id, name)) = answer
            .parse::<usize>()
            .ok()
            .and_then(|number| projects.get(number.checked_sub(1)?))
        {
            return Ok(ProjectChoice::Existing(id.clone(), name.clone()));
        }
    }
    Err(canceled())
}

/// Asks a yes or no question.
fn confirm<R: BufRead, W: Write>(
    report: &mut Report<'_, W>,
    input: &mut R,
    question: &str,
    default: bool,
) -> Result<bool, RuntimeError> {
    let answer = ask(report, input, question)?.to_ascii_lowercase();
    Ok(match answer.as_str() {
        "" => default,
        _ => answer.starts_with('y'),
    })
}

/// Prints a question and reads one answer line; the end of input cancels setup.
fn ask<R: BufRead, W: Write>(
    report: &mut Report<'_, W>,
    input: &mut R,
    question: &str,
) -> Result<String, RuntimeError> {
    write!(report.output, "{question}")?;
    report.output.flush()?;
    let mut line = String::new();
    if std::io::Read::take(&mut *input, 1024).read_line(&mut line)? == 0 {
        return Err(canceled());
    }
    Ok(line.trim().to_owned())
}

/// Reads one variable from env-file text, ignoring empty values.
fn env_value(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        let value = value.trim().trim_matches(['"', '\'']);
        (key.trim() == name && !value.is_empty()).then(|| value.to_owned())
    })
}

/// Appends missing `LogBrew` variables to the env file, never replacing a value.
fn env_change(project: &Project, project_id: &str) -> Option<FileChange> {
    let (path, text, service) = (
        project.env_file,
        project.env_text.as_str(),
        project.service.as_str(),
    );
    let mut added = String::new();
    for (name, value) in [
        (KEY_VARIABLE, KEY_PLACEHOLDER),
        ("LOGBREW_SERVICE_NAME", service),
        ("LOGBREW_PROJECT_ID", project_id),
    ] {
        let declared = text.lines().any(|line| {
            let line = line.trim();
            line.strip_prefix("export ")
                .unwrap_or(line)
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == name)
        });
        if !declared {
            added.extend([name, "=", value, "\n"]);
        }
    }
    if added.is_empty() {
        return None;
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Some(FileChange {
        path: path.to_owned(),
        before: (!text.is_empty()).then(|| text.to_owned()),
        after: format!("{text}{separator}{added}"),
    })
}

/// Keeps the env file out of Git when the project uses Git.
fn gitignore_change(dir: &Path, env_file: &str) -> Option<FileChange> {
    let before = recipe::read(dir, ".gitignore");
    if before.is_none() && !dir.join(".git").exists() {
        return None;
    }
    let text = before.clone().unwrap_or_default();
    let ignored = text.lines().map(str::trim).any(|line| {
        let line = line.trim_start_matches('/');
        line == env_file || line == ".env*" || (env_file == ".env.local" && line == ".env*.local")
    });
    if ignored {
        return None;
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Some(FileChange {
        path: String::from(".gitignore"),
        before,
        after: format!("{text}{separator}{env_file}\n"),
    })
}

/// Renders a unified-style diff; env files show only added lines, never existing values.
fn diff(change: &FileChange) -> String {
    let before = change
        .before
        .as_deref()
        .unwrap_or_default()
        .lines()
        .collect::<Vec<_>>();
    let after = change.after.lines().collect::<Vec<_>>();
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let context = if change.path.starts_with(".env") {
        0
    } else {
        2
    };
    let mut text = format!(
        "--- {}\n+++ b/{}\n",
        if change.before.is_some() {
            format!("a/{}", change.path)
        } else {
            String::from("/dev/null")
        },
        change.path
    );
    let lines = |text: &mut String, mark: char, slice: &[&str]| {
        for line in slice {
            text.push(mark);
            text.push_str(line);
            text.push('\n');
        }
    };
    lines(
        &mut text,
        ' ',
        &before[prefix.saturating_sub(context)..prefix],
    );
    lines(&mut text, '-', &before[prefix..before.len() - suffix]);
    lines(&mut text, '+', &after[prefix..after.len() - suffix]);
    let end = before.len() - suffix;
    lines(
        &mut text,
        ' ',
        &before[end..before.len().min(end + context)],
    );
    text
}

/// Writes one change if the file still matches what was shown, never through a symlink.
fn write_change(
    dir: &Path,
    change: &FileChange,
    replace: &dyn Fn(&str) -> String,
) -> Result<(), RuntimeError> {
    let path = safe_path(dir, change.path.as_str())?;
    if recipe::read(dir, change.path.as_str()) != change.before
        || (change.before.is_none() && std::fs::symlink_metadata(path.as_path()).is_ok())
    {
        return Err(RuntimeError::Unavailable {
            message: "a project file changed while setup ran",
            next: "rerun logbrew setup",
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = atomic_write_file::AtomicWriteFile::open(path.as_path())?;
    #[cfg(unix)]
    if change.before.is_none() && change.path.starts_with(".env") {
        use std::os::unix::fs::PermissionsExt as _;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(replace(change.after.as_str()).as_bytes())?;
    file.commit()?;
    Ok(())
}

/// Joins a planned path to the project directory, refusing any symlinked component.
fn safe_path(dir: &Path, relative: &str) -> Result<std::path::PathBuf, RuntimeError> {
    let mut path = dir.to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        if std::fs::symlink_metadata(path.as_path())
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(unsafe_file());
        }
    }
    Ok(path)
}

/// Runs the install command in the project directory; its output goes to stderr.
async fn run(dir: &Path, command_line: &[String]) -> bool {
    let Some((program, args)) = command_line.split_first() else {
        return false;
    };
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        let _command = command.arg("/C").arg(program);
        command
    } else {
        std::process::Command::new(program)
    };
    let _command = command
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(std::io::stderr()))
        .stderr(std::process::Stdio::inherit());
    tokio::task::spawn_blocking(move || command.status())
        .await
        .is_ok_and(|status| status.is_ok_and(|status| status.success()))
}

/// Sends one test event with the ingest key; the server reads it back before it answers.
///
/// Returns the verify step, or `None` when the event did not arrive.
async fn verify(
    env: &CliEnvironment,
    project_id: &str,
    key: &str,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let body = serde_json::json!({"ingest_key": key});
    for _attempt in 0..VERIFY_ATTEMPTS {
        let (status, value) = first_run_post(env, project_id, "test-event", Some(&body)).await?;
        let Some(object) = value
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .filter(|object| status == 200 && object.len() == 7)
        else {
            return Ok(None);
        };
        let text = |name: &str| object.get(name).and_then(serde_json::Value::as_str);
        let trace_id = text("trace_id").filter(|trace| {
            trace.len() == 32 && trace.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        let elapsed_ms = object.get("elapsed_ms").and_then(serde_json::Value::as_u64);
        let (Some(trace_id), Some(elapsed_ms), Some(_next), Some("ingest_key")) =
            (trace_id, elapsed_ms, text("next"), text("credential"))
        else {
            return Ok(None);
        };
        if text("project_id") != Some(project_id) {
            return Ok(None);
        }
        match (text("result"), object.get("event_id")) {
            (Some("passed"), Some(serde_json::Value::String(event_id)))
                if crate::ids::is_uuid(event_id) =>
            {
                return Ok(Some(serde_json::json!({
                    "step": "verify",
                    "status": "arrived",
                    "event_id": event_id,
                    "trace_id": trace_id,
                    "elapsed_ms": elapsed_ms,
                })));
            }
            (Some("failed"), Some(serde_json::Value::Null)) => {}
            _ => return Ok(None),
        }
    }
    Ok(None)
}

/// Loads the removable sample data; a project that already has events keeps none.
async fn sample_data(
    env: &CliEnvironment,
    project_id: &str,
) -> Result<serde_json::Value, RuntimeError> {
    let (status, value) = first_run_post(env, project_id, "sample-data", None).await?;
    let counts = value
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .filter(|object| {
            status == 200
                && object.len() == 6
                && object.get("project_id").and_then(serde_json::Value::as_str) == Some(project_id)
                && object.get("loaded") == Some(&serde_json::Value::Bool(true))
        })
        .and_then(|object| {
            ["issue_events", "logs", "trace_spans", "actions"]
                .into_iter()
                .map(|name| Some((name.to_owned(), object.get(name)?.as_u64()?.into())))
                .collect::<Option<serde_json::Map<_, _>>>()
        });
    Ok(match (status, counts) {
        (200, Some(counts)) => {
            serde_json::json!({"step": "sample_data", "status": "loaded", "counts": counts})
        }
        (422, _) => serde_json::json!({"step": "sample_data", "status": "skipped"}),
        _ => serde_json::json!({"step": "sample_data", "status": "failed"}),
    })
}

/// Posts to one project's first-run route with the account; returns the status and JSON body.
async fn first_run_post(
    env: &CliEnvironment,
    project_id: &str,
    route: &str,
    body: Option<&serde_json::Value>,
) -> Result<(u16, Option<serde_json::Value>), RuntimeError> {
    let origin =
        crate::http::normalized_origin(env.base_url.as_str()).ok_or(RuntimeError::Unavailable {
            message: "the configured API URL is invalid",
            next: "set LOGBREW_API_URL to an http or https API origin and retry",
        })?;
    let client = crate::http::api_client()?;
    let url = format!("{origin}/api/projects/{project_id}/{route}");
    let (response, _credential) =
        crate::auth::send_account_authenticated_with_refresh(&client, env, |client, credential| {
            let request = client.post(url.as_str()).bearer_auth(credential.token());
            match body {
                Some(body) => request.json(body),
                None => request,
            }
        })
        .await?;
    let status = response.status().as_u16();
    let value = crate::http::bounded_body(response, FIRST_RUN_RESPONSE_LIMIT)
        .await
        .ok()
        .and_then(|text| serde_json::from_str(text.as_str()).ok());
    Ok((status, value))
}

/// Derives a safe project and service name from the project directory.
fn service_name(dir: &Path) -> String {
    let name = std::fs::canonicalize(dir)
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(64)
        .collect::<String>();
    if name.is_empty() {
        String::from("app")
    } else {
        name
    }
}

/// No supported project in the current directory.
const fn no_project() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "no supported project found",
        next: super::EMPTY_NEXT_STEP,
    }
}

/// The person ended setup before it changed anything.
const fn canceled() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "setup canceled; no changes made",
        next: "rerun logbrew setup, or use logbrew setup --yes",
    }
}

/// A setup target is a symlink or unreadable.
const fn unsafe_file() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "setup does not write through symlinks or unreadable files",
        next: "replace the symlink with a regular file, then rerun logbrew setup",
    }
}

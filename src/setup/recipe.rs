//! Framework-specific setup changes: SDK install, init code, env file, and agent config.

use std::path::Path;

/// Variable every server integration reads for its ingest key.
pub(super) const KEY_VARIABLE: &str = "LOGBREW_SERVER_API_KEY";
/// Hosted MCP server URL.
const MCP_URL: &str = "https://mcp.logbrew.co/mcp";
/// Largest project file setup reads or edits.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// One file change, shown as a diff before it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileChange {
    /// Path relative to the project directory, with forward slashes.
    pub(super) path: String,
    /// Current contents, or `None` when setup creates the file.
    pub(super) before: Option<String>,
    /// Contents after the change.
    pub(super) after: String,
}

/// What setup does for one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    /// The project already has it.
    Present,
    /// Change these files.
    Edit(FileChange),
    /// Run this command in the project directory.
    Run(Vec<String>),
    /// The person or agent adds this code or declaration by hand.
    Manual(String),
    /// The framework needs nothing here.
    NotNeeded,
}

impl Action {
    /// Returns the file change when this step edits a file.
    pub(super) const fn edit(&self) -> Option<&FileChange> {
        match self {
            Self::Edit(change) => Some(change),
            Self::Present | Self::Run(_) | Self::Manual(_) | Self::NotNeeded => None,
        }
    }
}

/// Reads one small regular project file without following a symlink.
pub(super) fn read(dir: &Path, relative: &str) -> Option<String> {
    let path = dir.join(relative);
    let metadata = std::fs::symlink_metadata(path.as_path()).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Returns the plan's integration key, or its ecosystem for source packages.
pub(super) fn integration(plan: &serde_json::Value) -> &str {
    plan.get("integration")
        .or_else(|| plan.get("ecosystem"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

/// Returns the env file the framework conventionally keeps local secrets in.
pub(super) fn env_file(integration: &str) -> &'static str {
    if integration == "symfony" {
        ".env.local"
    } else {
        ".env"
    }
}

/// Returns whether the framework loads its env file without extra code.
pub(super) fn loads_env_file(integration: &str) -> bool {
    matches!(integration, "symfony" | "sveltekit")
}

/// Plans the SDK install from the install plan and the project manifest.
pub(super) fn install(dir: &Path, manifest: &str, plan: &serde_json::Value) -> Action {
    let names = package_names(plan);
    let declared = [manifest, "requirements.txt", "Pipfile"]
        .iter()
        .filter_map(|file| read(dir, file))
        .collect::<String>();
    if !names.is_empty() && names.iter().all(|name| declared.contains(name.as_str())) {
        return Action::Present;
    }
    if let Some(command) = plan
        .get("install_command")
        .and_then(serde_json::Value::as_str)
    {
        let mut argv = split_command(command);
        let venv = dir.join(".venv/bin/python");
        if argv.first().is_some_and(|program| program == "python3") && venv.is_file() {
            argv[0] = venv.to_string_lossy().into_owned();
        }
        return Action::Run(argv);
    }
    let declaration = plan
        .get("dependency_declaration")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let edit = match plan
        .get("package_manager")
        .and_then(serde_json::Value::as_str)
    {
        Some("maven") => read(dir, manifest).and_then(|text| maven_edit(&text, declaration)),
        Some("gradle" | "gradle-kotlin") => read(dir, manifest).and_then(|text| {
            insert_at(
                &text,
                |line| (line == "dependencies {").then(String::new),
                |_| (String::new(), format!("    {declaration}")),
            )
        }),
        _ => None,
    };
    edit.map_or_else(
        || Action::Manual(manual_install(plan, declaration)),
        |after| {
            Action::Edit(FileChange {
                path: manifest.to_owned(),
                before: read(dir, manifest),
                after,
            })
        },
    )
}

/// Lists the package names an install plan adds.
fn package_names(plan: &serde_json::Value) -> Vec<String> {
    let name = |value: &serde_json::Value| {
        value
            .as_str()
            .or_else(|| {
                ["name", "module", "id", "artifact_id"]
                    .iter()
                    .find_map(|key| value.get(key)?.as_str())
            })
            .map(str::to_owned)
    };
    plan.get("packages")
        .and_then(serde_json::Value::as_array)
        .map(|packages| packages.iter().filter_map(name).collect())
        .or_else(|| plan.get("package").and_then(name).map(|name| vec![name]))
        .unwrap_or_default()
}

/// Splits a plan command on spaces, keeping double-quoted words together.
fn split_command(command: &str) -> Vec<String> {
    let mut words = vec![String::new()];
    let mut quoted = false;
    for character in command.chars() {
        match character {
            '"' => quoted = !quoted,
            ' ' if !quoted => words.push(String::new()),
            _ => words
                .last_mut()
                .into_iter()
                .for_each(|word| word.push(character)),
        }
    }
    words.retain(|word| !word.is_empty());
    words
}

/// Adds the Maven dependency inside the least-indented `</dependencies>`.
fn maven_edit(text: &str, declaration: &str) -> Option<String> {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let (index, close) = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "</dependencies>")
        .min_by_key(|(_, line)| indent(line))?;
    let mut output = lines[..index].concat();
    for line in declaration.lines() {
        output.push_str(&close[..indent(close)]);
        output.push_str("  ");
        output.push_str(line);
        output.push('\n');
    }
    output.push_str(&lines[index..].concat());
    Some(output)
}

/// Builds manual install text for source and Swift packages.
fn manual_install(plan: &serde_json::Value, declaration: &str) -> String {
    match integration(plan) {
        "source" => format!(
            "Vendor include/LogBrew.h and src/ from {} at tag {} into the app target.",
            plan["package_url"].as_str().unwrap_or_default(),
            plan["release_tag"].as_str().unwrap_or_default()
        ),
        "swiftpm" => format!(
            "Add {declaration} to Package.swift, or add the package in Xcode, and link the LogBrew product."
        ),
        "cmake" => format!("Add this to CMakeLists.txt and link LogBrew::LogBrew:\n{declaration}"),
        _ => format!("Add this dependency:\n{declaration}"),
    }
}

/// Plans the init code where the framework expects it.
pub(super) fn init(dir: &Path, plan: &serde_json::Value, service: &str) -> Action {
    let edit = |path: &str, after: Option<String>| {
        let before = read(dir, path);
        if before
            .as_deref()
            .is_some_and(|text| text.to_ascii_lowercase().contains("logbrew"))
        {
            return Some(Action::Present);
        }
        after.map(|after| {
            Action::Edit(FileChange {
                path: path.to_owned(),
                before,
                after,
            })
        })
    };
    let integration = integration(plan);
    let found = match integration {
        "rails" => Some(Action::NotNeeded),
        "sveltekit" => {
            let extension = if dir.join("tsconfig.json").is_file() {
                "ts"
            } else {
                "js"
            };
            ["src/hooks.server.js", "src/hooks.server.ts"]
                .into_iter()
                .find(|file| std::fs::symlink_metadata(dir.join(file)).is_ok())
                .map_or_else(
                    || {
                        edit(
                            &format!("src/hooks.server.{extension}"),
                            Some(svelte_hooks(service)),
                        )
                    },
                    |file| edit(file, None),
                )
        }
        "react_express" => first_edit(
            &edit,
            dir,
            [
                "server",
                "app",
                "index",
                "src/server",
                "src/app",
                "src/index",
                "server/index",
            ]
            .into_iter()
            .flat_map(|stem| [format!("{stem}.js"), format!("{stem}.ts")]),
            express_edit,
        ),
        "flask" => first_edit(
            &edit,
            dir,
            [
                "app.py",
                "wsgi.py",
                "main.py",
                "app/__init__.py",
                "src/app.py",
            ]
            .map(String::from),
            |text| python_edit(text, "Flask(", "flask", "init_logbrew(app)", ""),
        ),
        "fastapi" => first_edit(
            &edit,
            dir,
            ["main.py", "app.py", "app/main.py", "src/main.py"].map(String::from),
            |text| {
                let init = "init_logbrew(app, api_key=os.environ[\"LOGBREW_SERVER_API_KEY\"], service_name=SERVICE)";
                python_edit(text, "FastAPI(", "fastapi", init, service)
            },
        ),
        "django" => django_settings(dir).and_then(|path| {
            let text = read(dir, path.as_str())?;
            edit(
                path.as_str(),
                Some(format!(
                    "{text}{}",
                    django_block(text.ends_with('\n'), service)
                )),
            )
        }),
        "symfony" => {
            let text = read(dir, "config/bundles.php");
            edit("config/bundles.php", text.as_deref().and_then(symfony_edit))
        }
        "aspnetcore" => {
            let text = read(dir, "Program.cs");
            edit("Program.cs", text.as_deref().and_then(aspnetcore_edit))
        }
        _ => None,
    };
    found.unwrap_or_else(|| Action::Manual(manual_init(integration, service)))
}

/// Returns the first candidate file that is already set up or can be edited.
fn first_edit(
    edit: &dyn Fn(&str, Option<String>) -> Option<Action>,
    dir: &Path,
    candidates: impl IntoIterator<Item = String>,
    change: impl Fn(&str) -> Option<String>,
) -> Option<Action> {
    candidates.into_iter().find_map(|path| {
        let text = read(dir, &path)?;
        edit(&path, change(&text))
    })
}

/// Inserts blocks before and after the first line `anchor` accepts, keeping its indent.
fn insert_at(
    text: &str,
    anchor: impl Fn(&str) -> Option<String>,
    blocks: impl Fn(&str) -> (String, String),
) -> Option<String> {
    let mut output = String::with_capacity(text.len() + 512);
    let mut done = false;
    for line in text.split_inclusive('\n') {
        let name = if done { None } else { anchor(line.trim()) };
        let Some(name) = name else {
            output.push_str(line);
            continue;
        };
        let indent = &line[..line.len() - line.trim_start().len()];
        let (before, after) = blocks(&name);
        push_block(&mut output, indent, &before);
        output.push_str(line);
        if !line.ends_with('\n') {
            output.push('\n');
        }
        push_block(&mut output, indent, &after);
        done = true;
    }
    done.then_some(output)
}

/// Appends each block line with the anchor's indent.
fn push_block(output: &mut String, indent: &str, block: &str) {
    for line in block.lines() {
        if !line.is_empty() {
            output.push_str(indent);
        }
        output.push_str(line);
        output.push('\n');
    }
}

/// Returns the variable assigned by a one-line call such as `app = Flask(__name__)`.
fn assignment(line: &str, call: &str) -> Option<String> {
    let line = ["const ", "let ", "var "]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .unwrap_or(line);
    let (name, value) = line.split_once('=')?;
    let (name, value) = (name.trim(), value.trim().trim_end_matches(';'));
    let identifier = !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '$'));
    (identifier && value.starts_with(call) && value.ends_with(')')).then(|| name.to_owned())
}

/// Adds the request middleware after the Express app is created.
fn express_edit(text: &str) -> Option<String> {
    let module = text.starts_with("import ") || text.contains("\nimport ");
    insert_at(
        text,
        |line| assignment(line, "express("),
        |app| {
            let import = if module {
                "import { logbrewMiddleware } from \"@logbrew/express\";"
            } else {
                "const { logbrewMiddleware } = require(\"@logbrew/express\");"
            };
            (
                import.to_owned(),
                format!(
                    "if (process.env.LOGBREW_SERVER_API_KEY) {{\n  {app}.use(logbrewMiddleware());\n}}"
                ),
            )
        },
    )
}

/// Initializes a Python framework integration after its app is created.
fn python_edit(text: &str, call: &str, package: &str, init: &str, service: &str) -> Option<String> {
    insert_at(
        text,
        |line| assignment(line, call),
        |app| {
            (
                format!("import os\nfrom logbrew_{package} import init_logbrew"),
                format!(
                    "if os.environ.get(\"LOGBREW_SERVER_API_KEY\"):\n    {}",
                    init.replace("(app", format!("({app}").as_str())
                        .replace("SERVICE", format!("{service:?}").as_str())
                ),
            )
        },
    )
}

/// Finds the Django settings file named in `manage.py`.
fn django_settings(dir: &Path) -> Option<String> {
    let manage = read(dir, "manage.py")?;
    let (_, rest) = manage.split_once("DJANGO_SETTINGS_MODULE")?;
    let module = rest.split(['"', '\'']).nth(2)?;
    module
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
        .then(|| format!("{}.py", module.replace('.', "/")))
}

/// Builds the block appended to Django settings.
fn django_block(ends_with_newline: bool, service: &str) -> String {
    format!(
        "{}\n# LogBrew\nimport os as _logbrew_os\n\nif _logbrew_os.environ.get(\"LOGBREW_SERVER_API_KEY\"):\n    \
         from logbrew_django import configure_logbrew\n    from logbrew_sdk import HttpTransport, LogBrewClient\n\n    \
         MIDDLEWARE = [\"logbrew_django.LogBrewDjangoMiddleware\", *MIDDLEWARE]\n    configure_logbrew(\n        \
         client=LogBrewClient.create(api_key=_logbrew_os.environ[\"LOGBREW_SERVER_API_KEY\"]),\n        \
         transport=HttpTransport(),\n        service_name={service:?},\n    )\n",
        if ends_with_newline { "" } else { "\n" }
    )
}

/// Registers the Symfony bundle before the closing bracket of `config/bundles.php`.
fn symfony_edit(text: &str) -> Option<String> {
    let lines = text.lines().collect::<Vec<_>>();
    let close = lines
        .iter()
        .rposition(|line| line.trim().starts_with("];"))?;
    let previous = lines[..close]
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())?;
    if !previous.trim_end().ends_with([',', '[']) {
        return None;
    }
    let mut output = lines[..close].join("\n");
    output.push_str("\n    LogBrew\\Symfony\\LogBrewBundle::class => ['all' => true],\n");
    output.push_str(&lines[close..].join("\n"));
    output.push('\n');
    Some(output)
}

/// Adds the ASP.NET Core integration to `Program.cs`.
fn aspnetcore_edit(text: &str) -> Option<String> {
    let builder = text
        .lines()
        .find_map(|line| assignment(line.trim(), "WebApplication.CreateBuilder("))?;
    let text = insert_at(
        text,
        |line| assignment(line, "WebApplication.CreateBuilder("),
        |_| (String::new(), format!("{builder}.AddLogBrew();")),
    )?;
    let build = format!("{builder}.Build(");
    let app = text
        .lines()
        .find_map(|line| assignment(line.trim(), &build))?;
    let routing = format!("{app}.UseRouting();");
    let anchor = |line: &str| {
        if text.lines().any(|line| line.trim() == routing) {
            (line == routing).then(String::new)
        } else {
            assignment(line, &build)
        }
    };
    let text = insert_at(&text, anchor, |_| {
        (String::new(), format!("{app}.UseLogBrew();"))
    })?;
    Some(format!("using LogBrew;\n{text}"))
}

/// Builds the `SvelteKit` server hooks file.
fn svelte_hooks(service: &str) -> String {
    format!(
        "import {{ env }} from \"$env/dynamic/private\";\nimport {{ createLogBrewSvelteContext, createLogBrewSvelteKitHooks }} from \"@logbrew/svelte\";\n\n\
         const logbrew = env.LOGBREW_SERVER_API_KEY\n  ? createLogBrewSvelteKitHooks(\n      createLogBrewSvelteContext({{\n        \
         serverApiKey: env.LOGBREW_SERVER_API_KEY,\n        context: {{ schemaVersion: 1, resource: {{ service: {{ name: {service:?} }} }} }}\n      }})\n    )\n  : undefined;\n\n\
         export const handle = ({{ event, resolve }}) =>\n  logbrew ? logbrew.handle({{ event, resolve }}) : resolve(event);\n\
         export const handleError = (input) => logbrew?.handleError(input);\n"
    )
}

/// Returns the documented init code for frameworks without a safe automatic edit.
fn manual_init(integration: &str, service: &str) -> String {
    const STARTUP: &str = "Create the client once at startup";
    const MOBILE: &str = "Pass the key from .env to the app through your build settings, then create the client once";
    let (intro, code) = match integration {
        "node" | "react_express" => (
            STARTUP,
            "const { createLogBrewNodeClient } = require(\"@logbrew/node\");\n\
             const logbrew = createLogBrewNodeClient();",
        ),
        "sveltekit" => {
            let hooks = svelte_hooks(service);
            return format!(
                "Add the LogBrew hooks to your existing src/hooks.server file:\n{hooks}"
            );
        }
        "python" | "django" | "flask" | "fastapi" => (
            STARTUP,
            "import os\nfrom logbrew_sdk import LogBrewClient\n\n\
             client = LogBrewClient.create(api_key=os.environ[\"LOGBREW_SERVER_API_KEY\"])",
        ),
        "go" => (
            "Create the client once in main",
            "transport, err := logbrew.NewHTTPTransport(logbrew.HTTPTransportConfig{})\n\
             client, err := logbrew.NewClient(logbrew.Config{APIKey: os.Getenv(\"LOGBREW_SERVER_API_KEY\")})\n\
             defer client.Shutdown(transport)",
        ),
        "java" | "spring" => (
            if integration == "spring" {
                "Create one client at startup and expose it as a @Bean"
            } else {
                STARTUP
            },
            "import co.logbrew.sdk.LogBrewClient;\n\nLogBrewClient client = LogBrewClient.create(\
             System.getenv(\"LOGBREW_SERVER_API_KEY\"), SERVICE, \"1.0.0\");",
        ),
        "php" | "symfony" => (
            STARTUP,
            "use LogBrew\\LogBrewClient;\n\n\
             $client = LogBrewClient::create(getenv('LOGBREW_SERVER_API_KEY'), SERVICE, '1.0.0');",
        ),
        "ruby" => (
            STARTUP,
            "require \"logbrew\"\n\nclient = LogBrew::Client.create(api_key: \
             ENV.fetch(\"LOGBREW_SERVER_API_KEY\"), sdk_name: SERVICE, sdk_version: \"1.0.0\")",
        ),
        "aspnetcore" => (
            "In Program.cs, add these after CreateBuilder and UseRouting",
            "using LogBrew;\nbuilder.AddLogBrew();\napp.UseLogBrew();",
        ),
        "swiftpm" => (
            MOBILE,
            "import LogBrew\n\nlet client = try LogBrewClient.create(apiKey: apiKey, \
             sdkName: \"logbrew-swift\", sdkVersion: \"1.0.0\")",
        ),
        "source" => (
            MOBILE,
            "#import \"LogBrew.h\"\n\nLBWConfig *config = [LBWConfig configWithAPIKey:apiKey];",
        ),
        _ => (
            STARTUP,
            "#include \"logbrew.hpp\"\n\nlogbrew::LogBrewClient client(logbrew::Config{\
             std::getenv(\"LOGBREW_SERVER_API_KEY\"), \"logbrew-cpp\", logbrew::version, 2});",
        ),
    };
    format!(
        "{intro}:\n{}",
        code.replace("SERVICE", &format!("{service:?}"))
    )
}

/// One agent client whose project-scoped MCP config setup can write.
pub(super) struct McpClient {
    /// Display name.
    pub(super) name: &'static str,
    /// Config file relative to the project directory.
    pub(super) path: &'static str,
    /// Key holding the server map.
    key: &'static str,
    /// Server entry for the hosted MCP server.
    entry: &'static str,
    /// Project or home entries that show the client is in use.
    markers: &'static [&'static str],
}

/// Agent clients with project-scoped MCP config.
const MCP_CLIENTS: [McpClient; 3] = [
    McpClient {
        name: "Claude Code",
        path: ".mcp.json",
        key: "mcpServers",
        entry: "{ \"type\": \"http\", \"url\": \"URL\" }",
        markers: &[".claude", "CLAUDE.md", ".mcp.json"],
    },
    McpClient {
        name: "Cursor",
        path: ".cursor/mcp.json",
        key: "mcpServers",
        entry: "{ \"url\": \"URL\" }",
        markers: &[".cursor"],
    },
    McpClient {
        name: "VS Code",
        path: ".vscode/mcp.json",
        key: "servers",
        entry: "{ \"type\": \"http\", \"url\": \"URL\" }",
        markers: &[".vscode"],
    },
];

/// Returns detected agent clients and the MCP config change each needs.
pub(super) fn mcp(dir: &Path, home: Option<&Path>) -> Vec<(&'static McpClient, Action)> {
    MCP_CLIENTS
        .iter()
        .filter(|client| {
            client.markers.iter().any(|marker| {
                std::iter::once(dir)
                    .chain(home)
                    .any(|root| std::fs::symlink_metadata(root.join(marker)).is_ok())
            })
        })
        .map(|client| (client, mcp_action(dir, client)))
        .collect()
}

/// Adds the hosted server to one client's config without touching other entries.
fn mcp_action(dir: &Path, client: &McpClient) -> Action {
    let entry = client.entry.replace("URL", MCP_URL);
    let manual = || {
        Action::Manual(format!(
            "Add \"logbrew\": {entry} under \"{}\" in {}.",
            client.key, client.path
        ))
    };
    let Some(before) = read(dir, client.path) else {
        if std::fs::symlink_metadata(dir.join(client.path)).is_ok() {
            return manual();
        }
        return Action::Edit(FileChange {
            path: client.path.to_owned(),
            before: None,
            after: format!(
                "{{\n  \"{}\": {{\n    \"logbrew\": {entry}\n  }}\n}}\n",
                client.key
            ),
        });
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&before) else {
        return manual();
    };
    let Some(servers) = value.get(client.key).and_then(serde_json::Value::as_object) else {
        return manual();
    };
    if servers.contains_key("logbrew") {
        return Action::Present;
    }
    let quoted = format!("\"{}\"", client.key);
    let open = before
        .find(quoted.as_str())
        .and_then(|start| before[start..].find('{').map(|offset| start + offset + 1));
    let after = open.map(|open| {
        let separator = if servers.is_empty() { "" } else { "," };
        format!(
            "{}\n    \"logbrew\": {entry}{separator}{}",
            &before[..open],
            &before[open..]
        )
    });
    after
        .filter(|after| {
            serde_json::from_str::<serde_json::Value>(after)
                .ok()
                .and_then(|value| value.get(client.key)?.get("logbrew").cloned())
                .is_some()
        })
        .map_or_else(manual, |after| {
            Action::Edit(FileChange {
                path: client.path.to_owned(),
                before: Some(before.clone()),
                after,
            })
        })
}

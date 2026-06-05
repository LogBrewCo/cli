//! Common CLI flag parsing.

use crate::{CliError, ISSUE_STATUS_FILTER_NEXT_STEP, ReadOptions};

/// Parsed common flags.
#[derive(Debug, Default)]
pub(crate) struct Flags {
    /// Output mode.
    output: OutputMode,
    /// Setup detection mode.
    setup: SetupDetection,
    /// Confirmation mode.
    confirmation: ConfirmationMode,
    /// Browser launch mode.
    browser: BrowserLaunch,
    /// Read endpoint filters.
    read: ReadOptions,
}

/// Command-specific flag policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlagScope {
    /// Browser login command.
    Login,
    /// Local logout command.
    Logout,
    /// SDK setup command.
    Setup,
    /// Local/API status command.
    Status,
    /// Installed CLI version command.
    Version,
    /// Historical read commands.
    Read,
    /// Live watch command.
    Watch,
    /// Explanation command.
    Explain,
    /// State mutation command.
    Set,
    /// `resolve` issue shortcut.
    Resolve,
    /// `close` issue shortcut.
    Close,
    /// `ignore` issue shortcut.
    Ignore,
    /// `reopen` issue shortcut.
    Reopen,
    /// `resolved` issue status shortcut.
    StatusResolved,
    /// `closed` issue status shortcut.
    StatusClosed,
    /// `ignored` issue status shortcut.
    StatusIgnored,
    /// `open` issue status shortcut.
    StatusOpen,
    /// `unresolved` issue status shortcut.
    StatusUnresolved,
}

impl FlagScope {
    /// Returns the command name shown in parse errors.
    const fn command(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Logout => "logout",
            Self::Setup => "setup",
            Self::Status => "status",
            Self::Version => "version",
            Self::Read => "read",
            Self::Watch => "watch",
            Self::Explain => "explain",
            Self::Set => "set",
            Self::Resolve => "resolve",
            Self::Close => "close",
            Self::Ignore => "ignore",
            Self::Reopen => "reopen",
            Self::StatusResolved => "resolved",
            Self::StatusClosed => "closed",
            Self::StatusIgnored => "ignored",
            Self::StatusOpen => "open",
            Self::StatusUnresolved => "unresolved",
        }
    }

    /// Returns command-specific help for parse errors.
    const fn help_next(self) -> &'static str {
        match self {
            Self::Login => "run logbrew login --help",
            Self::Logout => "run logbrew logout --help",
            Self::Setup => "run logbrew setup --help",
            Self::Status => "run logbrew status --help",
            Self::Version => "run logbrew version --help",
            Self::Read => "run logbrew read --help",
            Self::Watch => "run logbrew watch --help",
            Self::Explain => "run logbrew explain --help",
            Self::Set => "run logbrew set --help",
            Self::Resolve => "run logbrew resolve --help",
            Self::Close => "run logbrew close --help",
            Self::Ignore => "run logbrew ignore --help",
            Self::Reopen => "run logbrew reopen --help",
            Self::StatusResolved => "run logbrew resolved --help",
            Self::StatusClosed => "run logbrew closed --help",
            Self::StatusIgnored => "run logbrew ignored --help",
            Self::StatusOpen => "run logbrew open --help",
            Self::StatusUnresolved => "run logbrew unresolved --help",
        }
    }

    /// Returns command-specific help for unexpected positional arguments.
    fn unexpected_next(self, argument: &str) -> &'static str {
        match (self, argument) {
            (Self::Read, "trace-id") => "use --trace <trace_id> or --trace-id <trace_id>",
            (Self::Read, "trace" | "traces" | "span" | "spans") => {
                "use --trace <trace_id> or run logbrew trace <trace_id>"
            }
            (Self::Read, "env" | "environment") => {
                "use --environment <environment> or --env <environment>"
            }
            (Self::Read, "release") => "use --release <release>",
            (Self::Read, "project" | "project-id") => {
                "use --project <project_id> or --project-id <project_id>"
            }
            (Self::Read, "status") => ISSUE_STATUS_FILTER_NEXT_STEP,
            (Self::Read, "level") => "use --level trace, debug, info, warn, error, or fatal",
            (Self::Read, "search") => "use --search <text>",
            (Self::Read, "user" | "distinct-id") => {
                "use --user <distinct_id> or --distinct-id <distinct_id>"
            }
            (Self::Read, "name") => "use --name <name>",
            (Self::Read, "since") => "use --since <duration>",
            (Self::Read, "limit") => "use --limit with a positive whole number",
            (Self::Read, _) => "use --release <release> or run logbrew read --help",
            (Self::Login, _) => "run logbrew login --help",
            (Self::Logout, _) => "run logbrew logout --help",
            (Self::Setup, _) => "run logbrew setup --help",
            (Self::Status, _) => "run logbrew status --help",
            (Self::Version, _) => "run logbrew version --help",
            (Self::Watch, _) => "run logbrew watch --help",
            (Self::Explain, _) => "run logbrew explain --help",
            (Self::Set, _) => "run logbrew set --help",
            (Self::Resolve, _) => "run logbrew resolve --help",
            (Self::Close, _) => "run logbrew close --help",
            (Self::Ignore, _) => "run logbrew ignore --help",
            (Self::Reopen, _) => "run logbrew reopen --help",
            (Self::StatusResolved, _) => "run logbrew resolved --help",
            (Self::StatusClosed, _) => "run logbrew closed --help",
            (Self::StatusIgnored, _) => "run logbrew ignored --help",
            (Self::StatusOpen, _) => "run logbrew open --help",
            (Self::StatusUnresolved, _) => "run logbrew unresolved --help",
        }
    }

    /// Returns whether a flag kind is allowed for this command.
    const fn allows(self, kind: FlagKind) -> bool {
        match kind {
            FlagKind::Json => true,
            FlagKind::Setup => matches!(self, Self::Setup),
            FlagKind::Login => matches!(self, Self::Login),
            FlagKind::ReadFilter => matches!(self, Self::Read),
        }
    }

    /// Builds an unsupported-flag parse error for this scope.
    fn unsupported(self, flag: &str) -> CliError {
        CliError::UnsupportedFlag {
            flag: flag.to_owned(),
            command: self.command(),
            next: self.help_next(),
        }
    }

    /// Builds an unknown-flag parse error for this scope.
    fn unknown_flag(self, flag: &str) -> CliError {
        CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: self.help_next(),
        }
    }

    /// Builds an unexpected-argument parse error for this scope.
    fn unexpected_argument(self, argument: &str) -> CliError {
        CliError::UnexpectedArgument {
            argument: argument.to_owned(),
            command: self.command(),
            next: self.unexpected_next(argument),
        }
    }

    /// Rejects a flag if this scope does not support it.
    fn ensure_allows(self, kind: FlagKind, flag: &str) -> Result<(), CliError> {
        if self.allows(kind) {
            Ok(())
        } else {
            Err(self.unsupported(flag))
        }
    }
}

/// Known CLI flag categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagKind {
    /// Stable JSON output.
    Json,
    /// Setup-only behavior flags.
    Setup,
    /// Login-only behavior flags.
    Login,
    /// Historical read filter flags.
    ReadFilter,
}

impl Flags {
    /// Returns whether JSON output was requested.
    #[must_use]
    pub(crate) const fn is_json(&self) -> bool {
        self.output.is_json()
    }

    /// Returns whether automatic setup was requested.
    #[must_use]
    pub(crate) const fn is_auto(&self) -> bool {
        self.setup.is_auto()
    }

    /// Returns whether confirmation prompts should be skipped.
    #[must_use]
    pub(crate) const fn skip_prompts(&self) -> bool {
        self.confirmation.skip_prompts()
    }

    /// Returns whether the CLI should try to open a browser.
    #[must_use]
    pub(crate) const fn should_open_browser(&self) -> bool {
        self.browser.should_open()
    }

    /// Consumes flag state into read endpoint options.
    #[must_use]
    pub(crate) fn into_read_options(self) -> ReadOptions {
        self.read
    }
}

/// Parses common CLI flags.
pub(crate) fn parse_flags(args: &[String], scope: FlagScope) -> Result<Flags, CliError> {
    let mut flags = Flags::default();
    let mut seen = Vec::new();
    let mut index = 0;

    while let Some(flag) = args.get(index) {
        parse_one_flag(
            flag.as_str(),
            args,
            &mut index,
            scope,
            &mut flags,
            &mut seen,
        )?;
        index += 1;
    }

    Ok(flags)
}

/// Returns whether a bare positional is likely a forgotten read filter flag.
pub(crate) fn is_read_filter_word(value: &str) -> bool {
    matches!(
        value,
        "env"
            | "environment"
            | "release"
            | "project"
            | "project-id"
            | "trace"
            | "trace-id"
            | "traces"
            | "span"
            | "spans"
            | "status"
            | "level"
            | "search"
            | "user"
            | "distinct-id"
            | "name"
            | "since"
            | "limit"
    )
}

/// Parses one flag or positional argument.
fn parse_one_flag(
    flag: &str,
    args: &[String],
    index: &mut usize,
    scope: FlagScope,
    flags: &mut Flags,
    seen: &mut Vec<&'static str>,
) -> Result<(), CliError> {
    if parse_simple_flag(flag, scope, flags, seen)?
        || parse_read_filter(flag, args, index, scope, flags, seen)?
    {
        return Ok(());
    }
    if let Some((name, _)) = flag.split_once('=')
        && is_simple_flag(name)
    {
        return Err(scope.unsupported(flag));
    }
    if flag.starts_with('-') {
        Err(scope.unknown_flag(flag))
    } else {
        Err(scope.unexpected_argument(flag))
    }
}

/// Returns whether a flag is a valueless common flag.
pub(crate) fn is_simple_flag(flag: &str) -> bool {
    matches!(flag, "--json" | "--auto" | "--yes" | "--no-open")
}

/// Parses output, setup, confirmation, and login flags.
fn parse_simple_flag(
    flag: &str,
    scope: FlagScope,
    flags: &mut Flags,
    seen: &mut Vec<&'static str>,
) -> Result<bool, CliError> {
    match flag {
        "--json" => {
            scope.ensure_allows(FlagKind::Json, "--json")?;
            mark_seen(seen, "--json")?;
            flags.output = OutputMode::Json;
        }
        "--auto" => {
            scope.ensure_allows(FlagKind::Setup, "--auto")?;
            mark_seen(seen, "--auto")?;
            flags.setup = SetupDetection::Auto;
        }
        "--yes" => {
            scope.ensure_allows(FlagKind::Setup, "--yes")?;
            mark_seen(seen, "--yes")?;
            flags.confirmation = ConfirmationMode::Skip;
        }
        "--no-open" => {
            scope.ensure_allows(FlagKind::Login, "--no-open")?;
            mark_seen(seen, "--no-open")?;
            flags.browser = BrowserLaunch::PrintOnly;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Parses read filter flags.
fn parse_read_filter(
    flag: &str,
    args: &[String],
    index: &mut usize,
    scope: FlagScope,
    flags: &mut Flags,
    seen: &mut Vec<&'static str>,
) -> Result<bool, CliError> {
    let (flag, inline_value) = split_inline_value(flag);
    let Some(spec) = read_filter_spec(flag) else {
        return Ok(false);
    };
    let value = read_filter_value(args, index, scope, seen, spec, inline_value)?;
    apply_read_filter(&mut flags.read, spec.kind, value)?;
    Ok(true)
}

/// Read filter metadata used for validation and duplicate handling.
#[derive(Debug, Clone, Copy)]
struct ReadFilterSpec {
    /// Field populated by this flag.
    kind: ReadFilterKind,
    /// Canonical flag name used for duplicate detection.
    canonical_flag: &'static str,
    /// User-visible flag name used in errors.
    visible_flag: &'static str,
}

impl ReadFilterSpec {
    /// Builds one read filter spec.
    const fn new(
        kind: ReadFilterKind,
        canonical_flag: &'static str,
        visible_flag: &'static str,
    ) -> Self {
        Self {
            kind,
            canonical_flag,
            visible_flag,
        }
    }
}

/// Read option populated by a flag.
#[derive(Debug, Clone, Copy)]
enum ReadFilterKind {
    /// Action/event name filter.
    Name,
    /// Relative or absolute time filter.
    Since,
    /// Actor/distinct-id filter.
    User,
    /// Trace correlation filter.
    Trace,
    /// Log level filter.
    Level,
    /// Log message search filter.
    Search,
    /// Project id filter.
    Project,
    /// Release filter.
    Release,
    /// Environment filter.
    Environment,
    /// Issue status filter.
    Status,
    /// Result limit filter.
    Limit,
}

/// Resolves a raw flag name to read filter metadata.
fn read_filter_spec(flag: &str) -> Option<ReadFilterSpec> {
    let spec = match flag {
        "--name" => ReadFilterSpec::new(ReadFilterKind::Name, "--name", "--name"),
        "--since" => ReadFilterSpec::new(ReadFilterKind::Since, "--since", "--since"),
        "--user" => ReadFilterSpec::new(ReadFilterKind::User, "--user", "--user"),
        "--distinct-id" => ReadFilterSpec::new(ReadFilterKind::User, "--user", "--distinct-id"),
        "--trace" => ReadFilterSpec::new(ReadFilterKind::Trace, "--trace", "--trace"),
        "--trace-id" => ReadFilterSpec::new(ReadFilterKind::Trace, "--trace", "--trace-id"),
        "--level" => ReadFilterSpec::new(ReadFilterKind::Level, "--level", "--level"),
        "--search" => ReadFilterSpec::new(ReadFilterKind::Search, "--search", "--search"),
        "--project" => ReadFilterSpec::new(ReadFilterKind::Project, "--project", "--project"),
        "--project-id" => ReadFilterSpec::new(ReadFilterKind::Project, "--project", "--project-id"),
        "--release" => ReadFilterSpec::new(ReadFilterKind::Release, "--release", "--release"),
        "--environment" => ReadFilterSpec::new(
            ReadFilterKind::Environment,
            "--environment",
            "--environment",
        ),
        "--env" => ReadFilterSpec::new(ReadFilterKind::Environment, "--environment", "--env"),
        "--status" => ReadFilterSpec::new(ReadFilterKind::Status, "--status", "--status"),
        "--limit" => ReadFilterSpec::new(ReadFilterKind::Limit, "--limit", "--limit"),
        _ => return None,
    };
    Some(spec)
}

/// Applies one parsed read filter value.
fn apply_read_filter(
    read: &mut ReadOptions,
    kind: ReadFilterKind,
    value: String,
) -> Result<(), CliError> {
    match kind {
        ReadFilterKind::Name => read.name = Some(value),
        ReadFilterKind::Since => read.since = Some(value),
        ReadFilterKind::User => read.user = Some(value),
        ReadFilterKind::Trace => read.trace = Some(value),
        ReadFilterKind::Level => read.level = Some(normalize_log_level(&value)?),
        ReadFilterKind::Search => read.search = Some(value),
        ReadFilterKind::Project => read.project = Some(value),
        ReadFilterKind::Release => read.release = Some(value),
        ReadFilterKind::Environment => read.environment = Some(value),
        ReadFilterKind::Status => read.status = Some(normalize_status(&value)?),
        ReadFilterKind::Limit => read.limit = Some(validate_limit(&value)?),
    }
    Ok(())
}

/// Splits `--flag=value` while leaving ordinary flags untouched.
fn split_inline_value(flag: &str) -> (&str, Option<&str>) {
    flag.split_once('=')
        .map_or((flag, None), |(name, value)| (name, Some(value)))
}

/// Reads a value-taking read filter after validating policy and duplicates.
fn read_filter_value(
    args: &[String],
    index: &mut usize,
    scope: FlagScope,
    seen: &mut Vec<&'static str>,
    spec: ReadFilterSpec,
    inline_value: Option<&str>,
) -> Result<String, CliError> {
    scope.ensure_allows(FlagKind::ReadFilter, spec.visible_flag)?;
    mark_seen(seen, spec.canonical_flag)?;
    if let Some(value) = inline_value {
        return take_inline_value(value, spec.visible_flag);
    }
    *index += 1;
    take_value(args, *index, spec.visible_flag)
}

/// Records a flag and rejects duplicate occurrences.
fn mark_seen(seen: &mut Vec<&'static str>, flag: &'static str) -> Result<(), CliError> {
    if seen.contains(&flag) {
        return Err(CliError::DuplicateFlag {
            flag,
            next: duplicate_flag_next(flag),
        });
    }
    seen.push(flag);
    Ok(())
}

/// Returns the next step for a duplicate flag.
fn duplicate_flag_next(flag: &'static str) -> &'static str {
    match flag {
        "--json" => "use --json once",
        "--auto" => "use --auto once",
        "--yes" => "use --yes once",
        "--no-open" => "use --no-open once",
        "--name" => "use --name once",
        "--since" => "use --since once",
        "--user" => "use --user once",
        "--trace" => "use --trace once",
        "--level" => "use --level once",
        "--search" => "use --search once",
        "--project" => "use --project once",
        "--release" => "use --release once",
        "--environment" => "use --environment once",
        "--status" => "use --status once",
        "--limit" => "use --limit once",
        _ => "use the flag once",
    }
}

/// Normalizes human-friendly status aliases.
pub(crate) fn normalize_status(status: &str) -> Result<String, CliError> {
    match status.to_ascii_lowercase().as_str() {
        "open" | "unresolved" => Ok(String::from("unresolved")),
        "resolved" | "closed" => Ok(String::from("resolved")),
        "ignored" => Ok(String::from("ignored")),
        other => Err(CliError::UnknownStatus(other.to_owned())),
    }
}

/// Normalizes human-friendly log level aliases.
pub(crate) fn normalize_log_level(level: &str) -> Result<String, CliError> {
    match level.to_ascii_lowercase().as_str() {
        "trace" => Ok(String::from("trace")),
        "debug" => Ok(String::from("debug")),
        "info" | "information" => Ok(String::from("info")),
        "warn" | "warning" => Ok(String::from("warn")),
        "error" | "err" => Ok(String::from("error")),
        "fatal" => Ok(String::from("fatal")),
        other => Err(CliError::UnknownLogLevel(other.to_owned())),
    }
}

/// Validates a positive whole-number row limit.
fn validate_limit(limit: &str) -> Result<String, CliError> {
    let is_positive = limit.parse::<u32>().is_ok_and(|value| value > 0);
    if is_positive {
        Ok(limit.to_owned())
    } else {
        Err(CliError::InvalidLimit(limit.to_owned()))
    }
}

/// Takes a flag value from `args`.
fn take_value(args: &[String], index: usize, flag: &'static str) -> Result<String, CliError> {
    let value = args.get(index).ok_or_else(|| missing_flag_value(flag))?;
    if value.starts_with('-') {
        return Err(missing_flag_value(flag));
    }
    Ok(value.clone())
}

/// Takes a value from `--flag=value` syntax.
fn take_inline_value(value: &str, flag: &'static str) -> Result<String, CliError> {
    if value.is_empty() {
        return Err(missing_flag_value(flag));
    }
    Ok(value.to_owned())
}

/// Builds a parse error for flags that are missing values.
fn missing_flag_value(flag: &'static str) -> CliError {
    CliError::MissingFlagValue {
        flag,
        next: missing_flag_value_next(flag),
    }
}

/// Returns the next step for a flag missing its value.
fn missing_flag_value_next(flag: &'static str) -> &'static str {
    match flag {
        "--name" => "provide a value after --name",
        "--since" => "provide a value after --since",
        "--user" => "provide a value after --user",
        "--distinct-id" => "provide a value after --distinct-id",
        "--trace" => "provide a value after --trace",
        "--trace-id" => "provide a value after --trace-id",
        "--level" => "provide a value after --level",
        "--search" => "provide a value after --search",
        "--project" => "provide a value after --project",
        "--project-id" => "provide a value after --project-id",
        "--release" => "provide a value after --release",
        "--environment" => "provide a value after --environment",
        "--env" => "provide a value after --env",
        "--status" => "provide a value after --status",
        "--limit" => "provide a value after --limit",
        _ => "provide a value after the flag",
    }
}

/// Output mode selected by common flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum OutputMode {
    /// Human-readable output.
    #[default]
    Human,
    /// Machine-readable JSON output.
    Json,
}

impl OutputMode {
    /// Returns whether JSON output was requested.
    const fn is_json(self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Setup detection mode selected by common flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SetupDetection {
    /// Ask or infer without forced automatic setup.
    #[default]
    Manual,
    /// Automatically detect project setup.
    Auto,
}

impl SetupDetection {
    /// Returns whether automatic setup was requested.
    const fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Confirmation behavior selected by common flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ConfirmationMode {
    /// Prompt before changes.
    #[default]
    Prompt,
    /// Skip confirmation prompts.
    Skip,
}

impl ConfirmationMode {
    /// Returns whether confirmation prompts should be skipped.
    const fn skip_prompts(self) -> bool {
        matches!(self, Self::Skip)
    }
}

/// Browser launch behavior selected by common flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum BrowserLaunch {
    /// Open the login URL in a browser.
    #[default]
    Open,
    /// Print the login URL without opening a browser.
    PrintOnly,
}

impl BrowserLaunch {
    /// Returns whether the CLI should try to open a browser.
    const fn should_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

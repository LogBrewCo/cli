//! Support-ticket command grammar.

use super::unknown_resource;
use crate::{
    CliError, Command, HelpTopic, SupportContextReplyOptions, SupportTarget,
    SupportTicketCreateOptions, SupportTicketLifecycleStatus, SupportTicketListOptions,
};

/// Recovery shown for invalid support command shapes.
const SUPPORT_NEXT_STEP: &str = "run logbrew support --help";

/// One value-taking support create field.
#[derive(Clone, Copy)]
enum CreateField {
    /// Support category.
    Category,
    /// Concise summary.
    Title,
    /// Reproduction details.
    Description,
    /// Related project identifier.
    ProjectId,
    /// Affected environment.
    Environment,
    /// Affected runtime.
    Runtime,
    /// Affected framework.
    Framework,
    /// Affected SDK package.
    SdkPackage,
    /// Affected SDK version.
    SdkVersion,
    /// Affected release.
    Release,
    /// Related trace identifier.
    TraceId,
    /// Related event identifier.
    EventId,
}

/// Canonical metadata for one value-taking support create flag.
struct CreateFieldSpec {
    /// Destination field.
    field: CreateField,
    /// Stable duplicate-detection name.
    canonical_flag: &'static str,
    /// User-facing spelling.
    visible_flag: &'static str,
}

/// Parses the authenticated support-ticket workflow.
pub(super) fn parse_support(args: &[String]) -> Result<Command, CliError> {
    let Some((action, tail)) = args.split_first() else {
        return Ok(Command::Help {
            topic: HelpTopic::Support,
            json: false,
        });
    };
    if action == "--json" && tail.is_empty() {
        return Ok(Command::Help {
            topic: HelpTopic::Support,
            json: true,
        });
    }
    match action.as_str() {
        "create" => parse_create(tail),
        "list" | "tickets" => parse_list(tail),
        "show" | "ticket" => parse_detail(tail),
        "context" => parse_context_history(tail),
        "reply" => parse_context_reply(tail),
        "close" => parse_lifecycle(tail, SupportTicketLifecycleStatus::Closed, "support close"),
        "reopen" => parse_lifecycle(tail, SupportTicketLifecycleStatus::Open, "support reopen"),
        other => Err(unknown_resource(other, SUPPORT_NEXT_STEP)),
    }
}

/// Parses one support context history read.
fn parse_context_history(args: &[String]) -> Result<Command, CliError> {
    let (ticket_id, json) = parse_ticket_id_and_json(args)?;
    Ok(Command::Support {
        target: SupportTarget::ContextHistory { ticket_id },
        json,
    })
}

/// Parses one idempotent support context reply.
fn parse_context_reply(args: &[String]) -> Result<Command, CliError> {
    let Some((ticket_id, tail)) = args.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "ticket_id",
            next: "provide a support ticket id",
        });
    };
    if !crate::ids::is_support_ticket_id(ticket_id) {
        return Err(CliError::InvalidSupportTicketId);
    }

    let mut options = SupportContextReplyOptions {
        ticket_id: ticket_id.clone(),
        ..SupportContextReplyOptions::default()
    };
    let mut json = false;
    let mut seen = Vec::new();
    let mut index = 0;
    while let Some(raw) = tail.get(index) {
        let (flag, inline_value) = split_flag(raw);
        match flag {
            "--context" => {
                options.context = take_support_reply_value(
                    tail,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--context",
                    "--context",
                )?;
            }
            "--retry-key" | "--idempotency-key" => {
                options.retry_key = take_support_reply_value(
                    tail,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--retry-key",
                    if flag == "--idempotency-key" {
                        "--idempotency-key"
                    } else {
                        "--retry-key"
                    },
                )?;
            }
            "--diagnostics" => {
                reject_inline(inline_value, "--diagnostics")?;
                mark_seen(&mut seen, "--diagnostics")?;
                options.diagnostics = true;
            }
            "--json" => {
                reject_inline(inline_value, "--json")?;
                mark_seen(&mut seen, "--json")?;
                json = true;
            }
            _ => return Err(CliError::InvalidSupportContextReply),
        }
        index += 1;
    }
    let context = options.context.trim().to_owned();
    if !(1..=4000).contains(&context.chars().count()) {
        return Err(CliError::InvalidSupportContext);
    }
    options.context = context;
    if !(1..=128).contains(&options.retry_key.len())
        || !options
            .retry_key
            .bytes()
            .all(|byte| matches!(byte, b'!'..=b'~'))
    {
        return Err(CliError::InvalidSupportRetryKey);
    }
    Ok(Command::Support {
        target: SupportTarget::ReplyContext(Box::new(options)),
        json,
    })
}

/// Parses one exact public ticket id followed only by optional JSON output.
fn parse_ticket_id_and_json(args: &[String]) -> Result<(String, bool), CliError> {
    let Some((ticket_id, tail)) = args.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "ticket_id",
            next: "provide a support ticket id",
        });
    };
    if !crate::ids::is_support_ticket_id(ticket_id) {
        return Err(CliError::InvalidSupportTicketId);
    }
    let mut json = false;
    let mut seen = Vec::new();
    for value in tail {
        if value == "--json" {
            mark_seen(&mut seen, "--json")?;
            json = true;
        } else {
            return Err(CliError::InvalidSupportContextCommand);
        }
    }
    Ok((ticket_id.clone(), json))
}

/// Parses support-ticket creation flags.
fn parse_create(args: &[String]) -> Result<Command, CliError> {
    let mut options = SupportTicketCreateOptions::default();
    let mut json = false;
    let mut seen = Vec::new();
    let mut index = 0;
    while let Some(raw) = args.get(index) {
        let (flag, inline_value) = split_flag(raw);
        if let Some(spec) = create_field_spec(flag) {
            let value = take_flag_value(
                args,
                &mut index,
                inline_value,
                &mut seen,
                spec.canonical_flag,
                spec.visible_flag,
            )?;
            set_create_field(&mut options, spec.field, value);
            index += 1;
            continue;
        }
        match flag {
            "--json" => {
                reject_inline(inline_value, "--json")?;
                mark_seen(&mut seen, "--json")?;
                json = true;
            }
            "--diagnostics" => {
                reject_inline(inline_value, "--diagnostics")?;
                mark_seen(&mut seen, "--diagnostics")?;
                options.diagnostics = true;
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value => return Err(unexpected_argument(value, "support create")),
        }
        index += 1;
    }
    require_create_field(
        options.category.as_str(),
        "category",
        "provide --category with a supported support category",
    )?;
    require_create_field(
        options.title.as_str(),
        "title",
        "provide --title with a concise summary",
    )?;
    require_create_field(
        options.description.as_str(),
        "description",
        "provide --description with reproducible details",
    )?;
    validate_category(options.category.as_str())?;
    Ok(Command::Support {
        target: SupportTarget::Create(Box::new(options)),
        json,
    })
}

/// Resolves one value-taking create flag to its canonical field.
fn create_field_spec(flag: &str) -> Option<CreateFieldSpec> {
    let (field, canonical_flag) = match flag {
        "--category" => (CreateField::Category, "--category"),
        "--title" => (CreateField::Title, "--title"),
        "--description" => (CreateField::Description, "--description"),
        "--project" | "--project-id" => (CreateField::ProjectId, "--project"),
        "--environment" | "--env" => (CreateField::Environment, "--environment"),
        "--runtime" => (CreateField::Runtime, "--runtime"),
        "--framework" => (CreateField::Framework, "--framework"),
        "--sdk-package" => (CreateField::SdkPackage, "--sdk-package"),
        "--sdk-version" => (CreateField::SdkVersion, "--sdk-version"),
        "--release" => (CreateField::Release, "--release"),
        "--trace-id" => (CreateField::TraceId, "--trace-id"),
        "--event-id" => (CreateField::EventId, "--event-id"),
        _ => return None,
    };
    Some(CreateFieldSpec {
        field,
        canonical_flag,
        visible_flag: match flag {
            "--project-id" => "--project-id",
            "--env" => "--env",
            _ => canonical_flag,
        },
    })
}

/// Assigns one parsed support create field.
fn set_create_field(options: &mut SupportTicketCreateOptions, field: CreateField, value: String) {
    match field {
        CreateField::Category => options.category = value,
        CreateField::Title => options.title = value,
        CreateField::Description => options.description = value,
        CreateField::ProjectId => options.project_id = Some(value),
        CreateField::Environment => options.environment = Some(value),
        CreateField::Runtime => options.runtime = Some(value),
        CreateField::Framework => options.framework = Some(value),
        CreateField::SdkPackage => options.sdk_package = Some(value),
        CreateField::SdkVersion => options.sdk_version = Some(value),
        CreateField::Release => options.release = Some(value),
        CreateField::TraceId => options.trace_id = Some(value),
        CreateField::EventId => options.event_id = Some(value),
    }
}

/// Parses support-ticket history filters.
fn parse_list(args: &[String]) -> Result<Command, CliError> {
    let mut options = SupportTicketListOptions::default();
    let mut json = false;
    let mut seen = Vec::new();
    let mut index = 0;
    while let Some(raw) = args.get(index) {
        let (flag, inline_value) = split_flag(raw);
        match flag {
            "--json" => {
                reject_inline(inline_value, "--json")?;
                mark_seen(&mut seen, "--json")?;
                json = true;
            }
            "--project" | "--project-id" => {
                options.project_id = Some(take_flag_value(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--project",
                    if flag == "--project-id" {
                        "--project-id"
                    } else {
                        "--project"
                    },
                )?);
            }
            "--status" => {
                options.status = Some(take_support_string(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--status",
                )?);
            }
            "--source" => {
                options.source = Some(take_support_string(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--source",
                )?);
            }
            "--category" => {
                let category =
                    take_support_string(args, &mut index, inline_value, &mut seen, "--category")?;
                validate_category(category.as_str())?;
                options.category = Some(category);
            }
            "--release" => {
                options.release = Some(take_support_string(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--release",
                )?);
            }
            "--limit" => {
                let limit =
                    take_support_string(args, &mut index, inline_value, &mut seen, "--limit")?;
                if !limit.parse::<u32>().is_ok_and(|value| value > 0) {
                    return Err(CliError::InvalidLimit(limit));
                }
                options.limit = Some(limit);
            }
            "--pagination" => {
                let pagination =
                    take_support_string(args, &mut index, inline_value, &mut seen, "--pagination")?;
                if pagination != "cursor" {
                    return Err(CliError::UnknownPagination);
                }
                options.pagination = Some(pagination);
            }
            "--cursor-time" => {
                options.cursor_time = Some(take_support_string(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--cursor-time",
                )?);
            }
            "--cursor-id" => {
                options.cursor_id = Some(take_support_string(
                    args,
                    &mut index,
                    inline_value,
                    &mut seen,
                    "--cursor-id",
                )?);
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value => return Err(unexpected_argument(value, "support list")),
        }
        index += 1;
    }
    finish_list(options, json)
}

/// Validates and builds one support-ticket list command.
fn finish_list(options: SupportTicketListOptions, json: bool) -> Result<Command, CliError> {
    validate_cursor(&options)?;
    Ok(Command::Support {
        target: SupportTarget::List(Box::new(options)),
        json,
    })
}

/// Parses one support-ticket detail read.
fn parse_detail(args: &[String]) -> Result<Command, CliError> {
    let Some((ticket_id, tail)) = args.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "ticket_id",
            next: "provide a support ticket id",
        });
    };
    if ticket_id.starts_with('-') {
        return Err(CliError::MissingArgument {
            argument: "ticket_id",
            next: "provide a support ticket id",
        });
    }
    if !crate::ids::is_support_ticket_id(ticket_id) {
        return Err(CliError::InvalidSupportTicketId);
    }
    let mut json = false;
    let mut seen = Vec::new();
    for value in tail {
        if value == "--json" {
            mark_seen(&mut seen, "--json")?;
            json = true;
        } else if value.starts_with('-') {
            return Err(unknown_flag(value));
        } else {
            return Err(unexpected_argument(value, "support show"));
        }
    }
    Ok(Command::Support {
        target: SupportTarget::Detail(ticket_id.clone()),
        json,
    })
}

/// Parses one public support-ticket lifecycle update.
fn parse_lifecycle(
    args: &[String],
    status: SupportTicketLifecycleStatus,
    command: &'static str,
) -> Result<Command, CliError> {
    let Some((ticket_id, tail)) = args.split_first() else {
        return Err(CliError::MissingArgument {
            argument: "ticket_id",
            next: "provide a support ticket id",
        });
    };
    if !crate::ids::is_support_ticket_id(ticket_id) {
        return Err(CliError::InvalidSupportTicketId);
    }
    let mut json = false;
    let mut seen = Vec::new();
    for value in tail {
        if value == "--json" {
            mark_seen(&mut seen, "--json")?;
            json = true;
        } else if value.starts_with('-') {
            return Err(unknown_flag(value));
        } else {
            return Err(unexpected_argument(value, command));
        }
    }
    Ok(Command::Support {
        target: SupportTarget::UpdateStatus {
            ticket_id: ticket_id.clone(),
            status,
        },
        json,
    })
}

/// Validates the opt-in support cursor shape.
fn validate_cursor(options: &SupportTicketListOptions) -> Result<(), CliError> {
    if options
        .cursor_id
        .as_deref()
        .is_some_and(|value| !crate::ids::is_support_ticket_id(value))
    {
        return Err(CliError::InvalidSupportTicketId);
    }
    match (
        options.pagination.as_deref(),
        options.cursor_time.as_ref(),
        options.cursor_id.as_ref(),
    ) {
        (None | Some("cursor"), None, None) | (Some("cursor"), Some(_), Some(_)) => Ok(()),
        (None, _, _) => Err(CliError::InvalidSupportCursor(String::from(
            "cursor fields require --pagination cursor",
        ))),
        (Some("cursor"), _, _) => Err(CliError::InvalidSupportCursor(String::from(
            "--cursor-time and --cursor-id must be used together",
        ))),
        (Some(_), _, _) => Err(CliError::UnknownPagination),
    }
}

/// Validates one canonical support category without retaining invalid input.
fn validate_category(category: &str) -> Result<(), CliError> {
    if matches!(
        category,
        "sdk_install_failure"
            | "ingest_failure"
            | "auth_failure"
            | "project_setup"
            | "dashboard_issue"
            | "docs_confusion"
            | "cli_issue"
            | "mobile_issue"
            | "billing_question"
            | "other"
    ) {
        Ok(())
    } else {
        Err(CliError::UnknownSupportCategory)
    }
}

/// Requires a non-empty support create field.
fn require_create_field(
    value: &str,
    argument: &'static str,
    next: &'static str,
) -> Result<(), CliError> {
    if value.trim().is_empty() {
        Err(CliError::MissingArgument { argument, next })
    } else {
        Ok(())
    }
}

/// Reads one value-taking support flag.
fn take_support_string(
    args: &[String],
    index: &mut usize,
    inline_value: Option<&str>,
    seen: &mut Vec<&'static str>,
    flag: &'static str,
) -> Result<String, CliError> {
    take_flag_value(args, index, inline_value, seen, flag, flag)
}

/// Reads one support flag value after duplicate validation.
fn take_flag_value(
    args: &[String],
    index: &mut usize,
    inline_value: Option<&str>,
    seen: &mut Vec<&'static str>,
    canonical_flag: &'static str,
    visible_flag: &'static str,
) -> Result<String, CliError> {
    mark_seen(seen, canonical_flag)?;
    let value = inline_value.unwrap_or_else(|| {
        *index += 1;
        args.get(*index).map_or("", String::as_str)
    });
    if value.is_empty() || value.starts_with('-') {
        Err(CliError::MissingFlagValue {
            flag: visible_flag,
            next: missing_value_next(visible_flag),
        })
    } else {
        Ok(value.to_owned())
    }
}

/// Reads one context-reply value across the full public string domain.
fn take_support_reply_value(
    args: &[String],
    index: &mut usize,
    inline_value: Option<&str>,
    seen: &mut Vec<&'static str>,
    canonical_flag: &'static str,
    visible_flag: &'static str,
) -> Result<String, CliError> {
    mark_seen(seen, canonical_flag)?;
    let value = inline_value.unwrap_or_else(|| {
        *index += 1;
        args.get(*index).map_or("", String::as_str)
    });
    if value.is_empty() {
        Err(CliError::MissingFlagValue {
            flag: visible_flag,
            next: missing_value_next(visible_flag),
        })
    } else {
        Ok(value.to_owned())
    }
}

/// Rejects `--flag=value` for valueless flags.
fn reject_inline(value: Option<&str>, flag: &'static str) -> Result<(), CliError> {
    if value.is_some() {
        Err(CliError::UnknownFlag {
            flag: flag.to_owned(),
            next: SUPPORT_NEXT_STEP,
        })
    } else {
        Ok(())
    }
}

/// Records one canonical support flag.
fn mark_seen(seen: &mut Vec<&'static str>, flag: &'static str) -> Result<(), CliError> {
    if seen.contains(&flag) {
        Err(CliError::DuplicateFlag {
            flag,
            next: if flag == "--json" {
                "use --json once"
            } else {
                SUPPORT_NEXT_STEP
            },
        })
    } else {
        seen.push(flag);
        Ok(())
    }
}

/// Splits inline support flag values.
fn split_flag(value: &str) -> (&str, Option<&str>) {
    value
        .split_once('=')
        .map_or((value, None), |(flag, value)| (flag, Some(value)))
}

/// Builds a support-specific unknown flag error.
fn unknown_flag(flag: &str) -> CliError {
    CliError::UnknownFlag {
        flag: flag.to_owned(),
        next: SUPPORT_NEXT_STEP,
    }
}

/// Builds a support-specific unexpected positional error.
fn unexpected_argument(argument: &str, command: &'static str) -> CliError {
    CliError::UnexpectedArgument {
        argument: argument.to_owned(),
        command,
        next: SUPPORT_NEXT_STEP,
    }
}

/// Returns a concrete missing-value recovery for support flags.
fn missing_value_next(flag: &'static str) -> &'static str {
    match flag {
        "--category" => "provide a value after --category",
        "--title" => "provide a value after --title",
        "--description" => "provide a value after --description",
        "--project" => "provide a value after --project",
        "--project-id" => "provide a value after --project-id",
        "--environment" => "provide a value after --environment",
        "--env" => "provide a value after --env",
        "--runtime" => "provide a value after --runtime",
        "--framework" => "provide a value after --framework",
        "--sdk-package" => "provide a value after --sdk-package",
        "--sdk-version" => "provide a value after --sdk-version",
        "--release" => "provide a value after --release",
        "--trace-id" => "provide a value after --trace-id",
        "--event-id" => "provide a value after --event-id",
        "--status" => "provide a value after --status",
        "--source" => "provide a value after --source",
        "--limit" => "provide a value after --limit",
        "--pagination" => "provide a value after --pagination",
        "--cursor-time" => "provide a value after --cursor-time",
        "--cursor-id" => "provide a value after --cursor-id",
        "--context" => "provide a value after --context",
        "--retry-key" => "provide a value after --retry-key",
        "--idempotency-key" => "provide a value after --idempotency-key",
        _ => "provide a value after the flag",
    }
}

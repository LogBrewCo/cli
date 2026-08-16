//! Strict repository catalog and bounded component-discovery output.

#![expect(
    clippy::missing_docs_in_private_items,
    reason = "private wire fields mirror the validated public API contract"
)]

use std::collections::BTreeSet;

use crate::auth::AuthCredential;
use crate::{LoginProvider, RepositorySetupTarget, RuntimeError};

const PROVIDERS: [&str; 3] = ["github", "gitlab", "bitbucket"];
const LIMITATIONS: [&str; 12] = [
    "provider_tree_truncated",
    "entry_limit_reached",
    "depth_limit_reached",
    "manifest_limit_reached",
    "manifest_byte_limit_reached",
    "manifest_unreadable",
    "component_limit_reached",
    "no_supported_components_detected",
    "contents_authorization_required",
    "contents_permission_insufficient",
    "provider_unavailable",
    "provider_temporarily_unavailable",
];

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    providers: Vec<ProviderState>,
    repositories: Vec<Repository>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderState {
    provider: String,
    status: String,
    connect_href: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    provider: String,
    id: String,
    name: String,
    full_name: String,
    web_url: String,
    default_branch: Option<String>,
    #[serde(rename = "is_private")]
    _is_private: bool,
    languages: Vec<String>,
    runtime: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    response_version: u8,
    status: String,
    repository: Option<DiscoveredRepository>,
    contents_authorization: Authorization,
    snapshot: Option<Snapshot>,
    components: Vec<Component>,
    coverage: Option<Coverage>,
    limitations: Vec<String>,
    next: String,
    next_action: Action,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveredRepository {
    provider: String,
    id: String,
    full_name: String,
    default_branch: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    status: String,
    connect_href: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    id: String,
    revision: String,
    discovered_at: String,
    expires_at: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Component {
    id: String,
    path: String,
    service_name: String,
    runtime: String,
    framework: Option<String>,
    kind: String,
    recommended: bool,
    evidence: Vec<Evidence>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    kind: String,
    path: String,
    value: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    max_depth: u8,
    entry_limit: u32,
    entries_seen: u32,
    manifest_limit: u16,
    manifests_found: u16,
    manifests_read: u16,
    manifests_unreadable: u16,
    manifest_bytes_read: u32,
    component_limit: u16,
    components_detected: u16,
    provider_truncated: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    code: String,
    target: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelope {
    error: String,
    code: String,
    next: String,
    next_action: Action,
    retry_after_seconds: Option<u64>,
}

/// Validates and writes one successful repository setup response.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent renderer consumes this private-module helper"
)]
pub(crate) fn write_success<W: std::io::Write>(
    target: &RepositorySetupTarget,
    json: bool,
    body: &str,
    output: &mut W,
) -> Result<(), RuntimeError> {
    match target {
        RepositorySetupTarget::Catalog => {
            let value: Catalog = serde_json::from_str(body).map_err(|_| invalid_response())?;
            validate_catalog(&value)?;
            if json {
                writeln!(output, "{body}")?;
            } else {
                write_catalog(&value, output)?;
            }
        }
        RepositorySetupTarget::Discover(request) => {
            let value: Discovery = serde_json::from_str(body).map_err(|_| invalid_response())?;
            validate_discovery(&value, request.provider, request.repository_id.as_str())?;
            if json {
                writeln!(output, "{body}")?;
            } else {
                write_discovery(&value, output)?;
            }
        }
    }
    Ok(())
}

fn validate_catalog(value: &Catalog) -> Result<(), RuntimeError> {
    if value.providers.len() != PROVIDERS.len() || value.repositories.len() > 300 {
        return Err(invalid_response());
    }
    let mut connected = BTreeSet::new();
    for (state, expected) in value.providers.iter().zip(PROVIDERS) {
        let connect = state.connect_href.as_deref();
        if state.provider != expected
            || !matches!(
                state.status.as_str(),
                "connected" | "authorization_required" | "unavailable" | "error"
            )
            || connect.is_some()
                != matches!(state.status.as_str(), "authorization_required" | "error")
            || connect.is_some_and(|href| !valid_connect_href(href, expected, false))
        {
            return Err(invalid_response());
        }
        if state.status == "connected" {
            let _inserted = connected.insert(expected);
        }
    }
    let mut identities = BTreeSet::new();
    for repository in &value.repositories {
        let url = reqwest::Url::parse(repository.web_url.as_str()).ok();
        if !connected.contains(repository.provider.as_str())
            || !PROVIDERS.contains(&repository.provider.as_str())
            || !safe(&repository.id, 256, false)
            || !safe(&repository.name, 120, false)
            || !safe(&repository.full_name, 256, false)
            || url.is_none_or(|url| url.scheme() != "https" || url.host_str().is_none())
            || repository
                .default_branch
                .as_deref()
                .is_some_and(|value| !safe(value, 256, true))
            || repository
                .runtime
                .as_deref()
                .is_some_and(|value| !safe(value, 64, false))
            || repository.languages.len() > 64
            || repository
                .languages
                .iter()
                .any(|value| !safe(value, 64, false))
            || !identities.insert((repository.provider.as_str(), repository.id.as_str()))
        {
            return Err(invalid_response());
        }
    }
    if value.repositories.windows(2).any(|pair| {
        (
            pair[0].full_name.to_ascii_lowercase(),
            pair[0].provider.as_str(),
        ) > (
            pair[1].full_name.to_ascii_lowercase(),
            pair[1].provider.as_str(),
        )
    }) {
        return Err(invalid_response());
    }
    Ok(())
}

fn validate_discovery(
    value: &Discovery,
    provider: LoginProvider,
    repository_id: &str,
) -> Result<(), RuntimeError> {
    if value.response_version != 1
        || !safe(&value.next, 512, false)
        || value.limitations.len() > LIMITATIONS.len()
        || value
            .limitations
            .iter()
            .any(|item| !LIMITATIONS.contains(&item.as_str()))
        || value.limitations.windows(2).any(|pair| {
            LIMITATIONS.iter().position(|item| *item == pair[0])
                >= LIMITATIONS.iter().position(|item| *item == pair[1])
        })
    {
        return Err(invalid_response());
    }
    if let Some(repository) = value.repository.as_ref() {
        if repository.provider != provider.as_str()
            || repository.id != repository_id
            || !safe(&repository.full_name, 256, false)
            || !safe(&repository.default_branch, 256, true)
        {
            return Err(invalid_response());
        }
    }
    if matches!(value.status.as_str(), "complete" | "partial") {
        validate_complete_discovery(value)
    } else {
        validate_unavailable_discovery(value, provider)
    }
}

fn validate_complete_discovery(value: &Discovery) -> Result<(), RuntimeError> {
    let (Some(_repository), Some(snapshot), Some(coverage)) = (
        value.repository.as_ref(),
        value.snapshot.as_ref(),
        value.coverage.as_ref(),
    ) else {
        return Err(invalid_response());
    };
    let discovered = crate::time::parse_rfc3339(snapshot.discovered_at.as_str());
    let expires = crate::time::parse_rfc3339(snapshot.expires_at.as_str());
    let coverage_limit = value
        .limitations
        .iter()
        .any(|item| LIMITATIONS[..7].contains(&item.as_str()));
    if value.contents_authorization.status != "ready"
        || value.contents_authorization.connect_href.is_some()
        || !crate::ids::is_non_nil_uuid(snapshot.id.as_str())
        || !safe(&snapshot.revision, 256, false)
        || value
            .limitations
            .iter()
            .any(|item| LIMITATIONS[8..].contains(&item.as_str()))
        || discovered
            .zip(expires)
            .is_none_or(|(start, end)| start >= end)
        || (value.status == "partial") != coverage_limit
        || !valid_coverage(value, coverage)
    {
        return Err(invalid_response());
    }
    let action = if value.components.is_empty() {
        ("use_manual_project_setup", "project_creation")
    } else {
        ("select_repository_components", "project_creation")
    };
    if (
        value.next_action.code.as_str(),
        value.next_action.target.as_str(),
    ) != action
        || value.components.len() > 32
        || value.components.windows(2).any(|pair| {
            (pair[0].path.as_str(), pair[0].runtime.as_str())
                >= (pair[1].path.as_str(), pair[1].runtime.as_str())
        })
        || value
            .components
            .iter()
            .map(|item| item.id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != value.components.len()
        || value
            .components
            .iter()
            .map(|item| item.service_name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != value.components.len()
        || value
            .components
            .iter()
            .any(|component| !valid_component(component))
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn valid_coverage(value: &Discovery, coverage: &Coverage) -> bool {
    coverage.max_depth == 6
        && coverage.entry_limit == 5_000
        && coverage.manifest_limit == 48
        && coverage.component_limit == 32
        && coverage
            .manifests_read
            .saturating_add(coverage.manifests_unreadable)
            <= coverage.manifests_found.min(coverage.manifest_limit)
        && coverage.manifest_bytes_read <= 1_048_576
        && (coverage.entries_seen > coverage.entry_limit)
            == has_limitation(value, "entry_limit_reached")
        && (coverage.manifests_found > coverage.manifest_limit)
            == has_limitation(value, "manifest_limit_reached")
        && (coverage.manifests_unreadable > 0)
            == (has_limitation(value, "manifest_byte_limit_reached")
                || has_limitation(value, "manifest_unreadable"))
        && (coverage.components_detected > coverage.component_limit)
            == has_limitation(value, "component_limit_reached")
        && usize::from(coverage.components_detected.min(coverage.component_limit))
            == value.components.len()
        && value.components.is_empty() == has_limitation(value, "no_supported_components_detected")
        && coverage.provider_truncated == has_limitation(value, "provider_tree_truncated")
}

fn has_limitation(value: &Discovery, limitation: &str) -> bool {
    value.limitations.iter().any(|item| item == limitation)
}

fn valid_component(value: &Component) -> bool {
    crate::ids::is_non_nil_uuid(value.id.as_str())
        && valid_path(value.path.as_str())
        && safe(&value.service_name, 80, false)
        && safe(&value.runtime, 64, false)
        && value
            .framework
            .as_deref()
            .is_none_or(|item| safe(item, 64, false))
        && matches!(
            value.kind.as_str(),
            "application" | "service" | "library" | "workspace" | "unknown"
        )
        && value.evidence.len() <= 8
        && !value.evidence.windows(2).any(|pair| {
            (
                pair[0].path.as_str(),
                evidence_kind_order(pair[0].kind.as_str()),
                pair[0].value.as_str(),
            ) >= (
                pair[1].path.as_str(),
                evidence_kind_order(pair[1].kind.as_str()),
                pair[1].value.as_str(),
            )
        })
        && value.evidence.iter().all(|evidence| {
            matches!(
                evidence.kind.as_str(),
                "manifest" | "framework_dependency" | "framework_configuration"
            ) && valid_path(evidence.path.as_str())
                && safe(&evidence.value, 128, false)
        })
}

const fn evidence_kind_order(value: &str) -> u8 {
    match value.as_bytes() {
        b"manifest" => 0,
        b"framework_dependency" => 1,
        b"framework_configuration" => 2,
        _ => u8::MAX,
    }
}

fn validate_unavailable_discovery(
    value: &Discovery,
    provider: LoginProvider,
) -> Result<(), RuntimeError> {
    if value.snapshot.is_some() || value.coverage.is_some() || !value.components.is_empty() {
        return Err(invalid_response());
    }
    let expected = match value.status.as_str() {
        "authorization_required" => (
            "authorization_required",
            "contents_authorization_required",
            "authorize_repository_contents",
            "repository_contents_authorization",
        ),
        "permission_insufficient" => (
            "permission_insufficient",
            "contents_permission_insufficient",
            "reauthorize_repository_contents",
            "repository_contents_authorization",
        ),
        "unavailable" => (
            "unavailable",
            "provider_unavailable",
            "authorize_repository_contents",
            "repository_contents_authorization",
        ),
        "temporarily_unavailable" => (
            "error",
            "provider_temporarily_unavailable",
            "retry_repository_discovery",
            "repository_component_discovery",
        ),
        _ => return Err(invalid_response()),
    };
    let href = value.contents_authorization.connect_href.as_deref();
    let identity_shape = match value.status.as_str() {
        "authorization_required" => href.is_some(),
        "permission_insufficient" => value.repository.is_some() && href.is_some(),
        "unavailable" => value.repository.is_none() && href.is_none(),
        "temporarily_unavailable" => value.repository.is_some() != href.is_some(),
        _ => false,
    };
    if value.contents_authorization.status != expected.0
        || value.limitations.as_slice() != [expected.1]
        || value.next_action.code != expected.2
        || value.next_action.target != expected.3
        || !identity_shape
        || href.is_some_and(|href| !valid_connect_href(href, provider.as_str(), true))
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn valid_path(value: &str) -> bool {
    safe(value, 512, false)
        && !value.starts_with(['/', '\\'])
        && !value.contains('\\')
        && (value == "."
            || value
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..")))
}

fn valid_connect_href(value: &str, provider: &str, contents: bool) -> bool {
    let suffix = if contents { "/contents" } else { "" };
    value
        == format!(
            "/api/auth/web/repositories/{provider}{suffix}?return_to=%2Fdashboard%2Fprojects%3Fsetup%3D1"
        )
}

fn safe(value: &str, limit: usize, allow_empty: bool) -> bool {
    value.len() <= limit
        && (allow_empty || !value.trim().is_empty())
        && crate::http::display_safe(value, limit)
}

fn write_catalog<W: std::io::Write>(value: &Catalog, output: &mut W) -> std::io::Result<()> {
    writeln!(output, "Repository providers")?;
    for state in &value.providers {
        write!(output, "- {}: {}", state.provider, state.status)?;
        if let Some(href) = state.connect_href.as_deref() {
            write!(output, " authorization_path={href}")?;
        }
        writeln!(output)?;
    }
    writeln!(output, "Repositories ({})", value.repositories.len())?;
    for repository in value.repositories.iter().take(100) {
        writeln!(
            output,
            "- {} provider={} id={} runtime={}",
            repository.full_name,
            repository.provider,
            repository.id,
            repository.runtime.as_deref().unwrap_or("not_reported")
        )?;
    }
    if value.repositories.len() > 100 {
        writeln!(
            output,
            "Showing 100 of {}; use --json for all.",
            value.repositories.len()
        )?;
    }
    writeln!(
        output,
        "Next: run logbrew projects repositories discover --provider <provider> --repository <id>."
    )
}

fn write_discovery<W: std::io::Write>(value: &Discovery, output: &mut W) -> std::io::Result<()> {
    writeln!(output, "Repository component discovery: {}", value.status)?;
    writeln!(
        output,
        "Contents access: {}",
        value.contents_authorization.status
    )?;
    if let Some(snapshot) = value.snapshot.as_ref() {
        writeln!(
            output,
            "Snapshot: id={} revision={} expires={}",
            snapshot.id, snapshot.revision, snapshot.expires_at
        )?;
    }
    writeln!(output, "Components: {}", value.components.len())?;
    for component in &value.components {
        writeln!(
            output,
            "- {} id={} service={} runtime={} kind={} recommended={}",
            component.path,
            component.id,
            component.service_name,
            component.runtime,
            component.kind,
            component.recommended
        )?;
    }
    writeln!(
        output,
        "Limitations: {}",
        if value.limitations.is_empty() {
            "none".to_owned()
        } else {
            value.limitations.join(",")
        }
    )?;
    writeln!(
        output,
        "Next: {}",
        discovery_next(value.next_action.code.as_str())
    )?;
    if let Some(href) = value.contents_authorization.connect_href.as_deref() {
        writeln!(output, "Authorization path: {href}")?;
    }
    Ok(())
}

/// Maps one validated action to local, value-safe human recovery.
const fn discovery_next(code: &str) -> &'static str {
    match code.as_bytes() {
        b"authorize_repository_contents" => "authorize read-only repository contents, then retry",
        b"reauthorize_repository_contents" => "renew repository contents-read access, then retry",
        b"retry_repository_discovery" => "retry repository component discovery",
        b"select_repository_components" => "select components and create the project",
        b"use_manual_project_setup" => "continue with manual project setup",
        _ => "retry repository setup",
    }
}

/// Replaces a validated API error with fixed local text.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent HTTP executor consumes this private-module helper"
)]
pub(crate) fn validate_error(
    status: u16,
    body: &str,
    credential: &AuthCredential,
) -> Result<RuntimeError, RuntimeError> {
    let value: ErrorEnvelope = serde_json::from_str(body).map_err(|_| invalid_response())?;
    if !safe(&value.error, 512, false) || !safe(&value.next, 512, false) {
        return Err(invalid_response());
    }
    let valid = match status {
        401 => {
            value.code == "unauthorized"
                && value.next_action.code == "sign_in"
                && value.next_action.target == "auth"
        }
        404 => {
            value.code == "not_found"
                && value.next_action.code == "check_resource"
                && value.next_action.target == "resource"
        }
        405 => {
            value.code == "method_not_allowed"
                && value.next_action.code == "use_supported_method"
                && value.next_action.target == "api_method"
        }
        422 => {
            matches!(value.code.as_str(), "validation_failed" | "invalid_json")
                && matches!(
                    value.next_action.code.as_str(),
                    "fix_request" | "send_valid_json"
                )
                && value.next_action.target == "request"
        }
        429 => {
            value.code == "rate_limited"
                && value.next_action.code == "retry_with_backoff"
                && value.next_action.target == "request"
                && value.retry_after_seconds.is_some()
        }
        500..=599 => matches!(
            (
                value.code.as_str(),
                value.next_action.code.as_str(),
                value.next_action.target.as_str()
            ),
            ("storage_error", "retry_or_check_storage", "backend_status")
                | ("json_error", "send_valid_json", "request")
                | ("internal_error", "retry_or_contact_support", "support")
        ),
        _ => false,
    };
    if !valid || (status != 429 && value.retry_after_seconds.is_some()) {
        return Err(invalid_response());
    }
    Ok(RuntimeError::Api {
        status,
        body: serde_json::json!({
            "error": "repository setup request failed",
            "code": value.code,
            "next": "retry the repository setup command after correcting the reported state",
            "next_action": {
                "code": value.next_action.code,
                "target": value.next_action.target,
            }
        })
        .to_string(),
        auth_source: credential.source(),
        auth_label: credential.label(),
    })
}

const fn invalid_response() -> RuntimeError {
    RuntimeError::Unavailable {
        message: "repository setup response was invalid",
        next: "retry the repository setup command; if it repeats, report the public response contract",
    }
}

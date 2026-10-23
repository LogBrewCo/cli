//! Grammar for native debug artifact upload and lookup.

use super::{is_canonical_lower_uuid, move_leading_json_to_tail};
use crate::{CliError, Command};

/// Parses the closed Apple native debug-artifact grammar.
pub(super) fn parse_native_debug_artifacts(args: &[String]) -> Result<Command, CliError> {
    let normalized = move_leading_json_to_tail(args);
    let Some((operation, tail)) = normalized.split_first() else {
        return Err(CliError::InvalidNativeDebugCommand);
    };
    match operation.as_str() {
        "upload" => parse_native_debug_upload(tail),
        "lookup" => parse_native_debug_lookup(tail),
        _ => Err(CliError::InvalidNativeDebugCommand),
    }
}

/// Parses one artifact upload and normalizes its public request scope.
fn parse_native_debug_upload(args: &[String]) -> Result<Command, CliError> {
    let Some((path, flags)) = args.split_first() else {
        return Err(CliError::InvalidNativeDebugCommand);
    };
    if path.is_empty() || path.chars().any(char::is_control) || path.starts_with('-') {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    let parsed = parse_native_debug_scope(flags, false)?;
    Ok(Command::NativeDebugArtifacts {
        target: crate::NativeDebugArtifactsTarget::Upload(crate::NativeDebugUploadOptions {
            path: path.clone(),
            project_id: parsed.project_id,
            release: parsed.release,
            environment: parsed.environment,
            service: parsed.service,
            expected_image_uuids: parsed.expected_image_uuids,
            dry_run: parsed.dry_run,
        }),
        json: parsed.json,
    })
}

/// Parses one exact artifact lookup.
fn parse_native_debug_lookup(args: &[String]) -> Result<Command, CliError> {
    let parsed = parse_native_debug_scope(args, true)?;
    let image_uuid = parsed
        .image_uuid
        .and_then(|value| normalize_native_debug_uuid(value.as_str()))
        .ok_or(CliError::InvalidNativeDebugIdentity)?;
    let architecture = parsed
        .architecture
        .filter(|value| ["arm", "arm64", "arm64e", "x86", "x86_64"].contains(&value.as_str()))
        .ok_or(CliError::InvalidNativeDebugIdentity)?;
    Ok(Command::NativeDebugArtifacts {
        target: crate::NativeDebugArtifactsTarget::Lookup(crate::NativeDebugLookupOptions {
            project_id: parsed.project_id,
            release: parsed.release,
            environment: parsed.environment,
            service: parsed.service,
            image_uuid,
            architecture,
        }),
        json: parsed.json,
    })
}

/// Duplicate-aware native debug-artifact flag accumulator.
#[derive(Default)]
struct NativeDebugScope {
    /// Account-owned project UUID.
    project_id: String,
    /// Exact normalized release.
    release: String,
    /// Exact normalized environment.
    environment: String,
    /// Exact normalized service.
    service: String,
    /// Optional lookup image UUID.
    image_uuid: Option<String>,
    /// Optional exact image UUID set for upload gating.
    expected_image_uuids: Vec<String>,
    /// Optional lookup architecture.
    architecture: Option<String>,
    /// Local-only artifact validation.
    dry_run: bool,
    /// Machine-readable output selection.
    json: bool,
}

/// Parses required scope flags without reflecting malformed values.
fn parse_native_debug_scope(args: &[String], lookup: bool) -> Result<NativeDebugScope, CliError> {
    let mut project_id = None;
    let mut release = None;
    let mut environment = None;
    let mut service = None;
    let mut image_uuid = None;
    let mut expected_image_uuids = Vec::new();
    let mut architecture = None;
    let mut dry_run = false;
    let mut json = false;
    let mut index = 0;
    while let Some(flag) = args.get(index) {
        if flag == "--json" {
            if json {
                return Err(CliError::InvalidNativeDebugCommand);
            }
            json = true;
            index += 1;
            continue;
        }
        if flag == "--dry-run" && !lookup {
            if dry_run {
                return Err(CliError::InvalidNativeDebugCommand);
            }
            dry_run = true;
            index += 1;
            continue;
        }
        if flag == "--expect-image-uuid" && !lookup {
            let value = args
                .get(index + 1)
                .and_then(|value| normalize_native_debug_uuid(value.as_str()))
                .ok_or(CliError::InvalidNativeDebugIdentity)?;
            if expected_image_uuids
                .iter()
                .any(|existing| existing == &value)
            {
                return Err(CliError::InvalidNativeDebugIdentity);
            }
            expected_image_uuids.push(value);
            index += 2;
            continue;
        }
        let destination = match flag.as_str() {
            "--project" => &mut project_id,
            "--release" => &mut release,
            "--environment" => &mut environment,
            "--service" => &mut service,
            "--image-uuid" if lookup => &mut image_uuid,
            "--architecture" if lookup => &mut architecture,
            _ => return Err(CliError::InvalidNativeDebugCommand),
        };
        if destination.is_some() {
            return Err(CliError::InvalidNativeDebugCommand);
        }
        let value = args
            .get(index + 1)
            .ok_or(CliError::InvalidNativeDebugCommand)?;
        *destination = Some(value.clone());
        index += 2;
    }

    let project_id = project_id
        .filter(|value| is_canonical_lower_uuid(value))
        .ok_or(CliError::InvalidNativeDebugCommand)?;
    let release = normalize_native_scope(release).ok_or(CliError::InvalidNativeDebugCommand)?;
    let environment =
        normalize_native_scope(environment).ok_or(CliError::InvalidNativeDebugCommand)?;
    let service = normalize_native_scope(service).ok_or(CliError::InvalidNativeDebugCommand)?;
    if lookup != (image_uuid.is_some() && architecture.is_some()) {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    if lookup && (!expected_image_uuids.is_empty() || dry_run) {
        return Err(CliError::InvalidNativeDebugCommand);
    }
    expected_image_uuids.sort();
    Ok(NativeDebugScope {
        project_id,
        release,
        environment,
        service,
        image_uuid,
        expected_image_uuids,
        architecture,
        dry_run,
        json,
    })
}

/// Trims and bounds one public native artifact scope string.
fn normalize_native_scope(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed.len() <= 256 && !trimmed.chars().any(char::is_control))
        .then(|| trimmed.to_owned())
}

/// Validates and canonicalizes one public native debug UUID.
fn normalize_native_debug_uuid(value: &str) -> Option<String> {
    if value.len() != 36 {
        return None;
    }
    let mut normalized = String::with_capacity(36);
    for (index, byte) in value.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
            normalized.push('-');
        } else if byte.is_ascii_hexdigit() {
            normalized.push(char::from(byte.to_ascii_lowercase()));
        } else {
            return None;
        }
    }
    Some(normalized)
}

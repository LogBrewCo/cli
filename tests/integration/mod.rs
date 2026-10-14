//! Complete public CLI integration contract.

mod action_investigation;
mod analytics_compare;
mod analytics_funnel;
mod analytics_lifecycle;
mod analytics_overview;
mod analytics_paths;
mod analytics_properties;
mod analytics_retention;
mod confidentiality;
mod deployment;
mod execution_commands;
mod hosted_runners;
mod login_loopback;
mod logout_sessions;
mod native_debug_artifact_upload;
mod native_debug_artifact_upload_bounds;
mod native_debug_artifacts;
mod package_contents;
mod project_archive;
mod project_doctor;
mod project_ingest_key_create;
mod projects;
mod release_workflows;
mod runtime_errors;
mod status_commands;
mod support_context;
mod support_tickets;
mod trace_discovery;
mod usage;
mod whoami;

pub(crate) use crate::support::*;

//! Recent trace discovery and trace-detail command parsing.

use crate::flags::{Flags, parse_trace_flags};
use crate::{CliError, Command, ExplainTarget, ReadTarget};

use super::{
    READ_TRACE_NEXT_STEP, READ_TRACES_NEXT_STEP, TRACE_DETAIL_UNSUPPORTED_FLAGS,
    parse_detail_explain_suffix, parse_detail_read_flags, reject_unsupported_read_flags,
    take_required_position, validate_read_filters,
};

/// Filters recent trace discovery cannot apply.
const TRACE_LIST_UNSUPPORTED_FLAGS: &[&str] = &[
    "--name",
    "--user",
    "--distinct-id",
    "--trace",
    "--trace-id",
    "--level",
    "--severity",
    "--search",
];

/// Parses recent trace discovery with its target-specific status vocabulary.
pub(super) fn parse_trace_list_read(rest: &[String]) -> Result<(ReadTarget, Flags), CliError> {
    reject_unsupported_read_flags(
        rest,
        "read traces",
        READ_TRACES_NEXT_STEP,
        TRACE_LIST_UNSUPPORTED_FLAGS,
    )?;
    let flags = match parse_trace_flags(rest) {
        Ok(flags) => flags,
        Err(CliError::UnexpectedArgument { argument, .. }) => {
            return Err(CliError::UnexpectedArgument {
                argument,
                command: "read traces",
                next: READ_TRACES_NEXT_STEP,
            });
        }
        Err(error) => return Err(error),
    };
    Ok((ReadTarget::Traces, flags))
}

/// Parses one trace detail read or trace explain suffix.
pub(super) fn parse_trace_detail_or_explain(rest: &[String]) -> Result<Command, CliError> {
    let (id, tail) = take_required_position(rest, "trace_id", "provide a trace id")?;
    if let Some(command) =
        parse_detail_explain_suffix(ExplainTarget::Trace(id.clone()), tail.as_slice())?
    {
        return Ok(command);
    }
    let target = ReadTarget::Trace(id);
    let flags = parse_detail_read_flags(
        tail.as_slice(),
        "read trace",
        READ_TRACE_NEXT_STEP,
        TRACE_DETAIL_UNSUPPORTED_FLAGS,
    )?;
    let json = flags.is_json();
    let options = flags.into_read_options();
    validate_read_filters(&target, &options)?;

    Ok(Command::Read {
        target,
        options: Box::new(options),
        json,
    })
}

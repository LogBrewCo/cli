//! Local CLI version output.

use crate::RuntimeError;

/// Executes installed CLI version output.
pub(crate) fn execute_version<W: std::io::Write>(
    json: bool,
    output: &mut W,
) -> Result<(), RuntimeError> {
    if json {
        let body = serde_json::json!({
            "ok": true,
            "name": "logbrew",
            "version": env!("CARGO_PKG_VERSION"),
            "binary": "native",
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        });
        writeln!(output, "{body}")?;
    } else {
        writeln!(output, "logbrew {}", env!("CARGO_PKG_VERSION"))?;
    }
    Ok(())
}

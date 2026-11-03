//! Built-binary TLS trust-root tests for Linux hosts with and without a CA store.

use crate::{Mock, MockServer, ResponseTemplate, TestResult, cli_command, run_cli_command};

/// Runs the built binary with one certificate-location variable set.
async fn run_with_cert_env(
    server: &MockServer,
    args: &[&str],
    variable: &str,
    value: &std::path::Path,
) -> TestResult<std::process::Output> {
    let mut command = cli_command(server);
    let _command = command
        .env("HOME", crate::isolated_home("logbrew-tls", variable)?)
        .env(variable, value)
        .args(args);
    run_cli_command(command).await
}

async_test!(empty_system_store_falls_back_to_bundled_roots -> TestResult, {
    let server = MockServer::start().await?;
    Mock::route("GET", "/health")
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server);
    Mock::route("GET", "/api/auth/account")
        .respond_with(ResponseTemplate::new(200).set_body_json(&serde_json::json!({
            "id": "00000000-0000-4000-8000-000000000001"
        })))
        .mount(&server);
    let empty_store = crate::isolated_home("logbrew-tls", "empty-store")?;

    let process =
        run_with_cert_env(&server, &["status", "--json"], "SSL_CERT_DIR", &empty_store).await?;

    crate::assert_cli_success(&process);
    let stdout: serde_json::Value = serde_json::from_slice(process.stdout.as_slice())?;
    assert_eq!(stdout["status"], "reachable", "client built from bundled roots");
    Ok(())
});

async_test!(unreadable_cert_file_names_the_fix -> TestResult, {
    let server = MockServer::start().await?;
    let missing = std::env::temp_dir().join("logbrew-tls-missing-bundle.pem");
    for args in [&["status"][..], &["issues"], &["issues", "--json"]] {
        let process = run_with_cert_env(&server, args, "SSL_CERT_FILE", &missing).await?;

        assert!(!process.status.success(), "{args:?} succeeded without trust roots");
        let stderr = String::from_utf8(process.stderr)?;
        assert!(!stderr.contains("builder error"), "{args:?} printed a bare builder error");
        if args.contains(&"--json") {
            let error: serde_json::Value = serde_json::from_str(&stderr)?;
            assert_eq!(error["message"], "TLS root certificates could not be loaded");
            assert_eq!(error["next"], "fix SSL_CERT_FILE or SSL_CERT_DIR, or install ca-certificates");
        } else {
            assert_eq!(
                stderr,
                "TLS root certificates could not be loaded\nNext: fix SSL_CERT_FILE or SSL_CERT_DIR, or install ca-certificates\n"
            );
        }
    }
    assert!(server.received_requests().is_empty(), "no request left without trust roots");
    Ok(())
});

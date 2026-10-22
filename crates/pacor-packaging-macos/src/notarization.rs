//! Apple notarization and ticket stapling.
//!
//! `notarytool submit --wait` exits 0 whether Apple accepted the upload or rejected
//! it, so the exit code alone cannot decide whether a build may be shipped. The
//! verdict is read from the JSON the submission prints, and a rejection is reported
//! with its submission id — the one thing needed to ask Apple why
//! (`notarytool log <id>`). Without this, a rejected upload surfaced only as the
//! failure of the `stapler` run that followed, which says nothing about the cause.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use pacor_core::utils::command::{
    CommandOutputMode, execute_command, execute_command_capture_stdout,
    macros::{command_args, command_args_push},
};

/// The status `notarytool` reports for a submission that may be stapled.
const ACCEPTED_STATUS: &str = "Accepted";

/// The fields of `notarytool submit --output-format json` this module reads.
///
/// All of them are optional because the shape changes with the outcome: a
/// submission that ran out of time reports an id and a message and no status.
#[derive(Debug, Default, Deserialize)]
struct Submission {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// The argument vector for `notarytool submit`.
fn submit_arguments(
    keychain_profile: &str,
    dmg_path: &Path,
    command_output_mode: &CommandOutputMode,
) -> Vec<OsString> {
    let mut args = command_args!(
        capacity = 10,
        "notarytool",
        "submit",
        dmg_path,
        "--keychain-profile",
        keychain_profile,
        "--wait",
        "--output-format",
        "json",
    );
    if command_output_mode == &CommandOutputMode::Verbose {
        command_args_push!(args, "--verbose");
    }
    args
}

/// Read the submission result out of the JSON `notarytool` printed.
fn parse_submission(json: &str) -> Result<Submission> {
    serde_json::from_str(json)
        .with_context(|| format!("could not read the notarization result: {}", json.trim()))
}

/// Fail unless the submission was accepted, naming the id that explains why not.
fn ensure_accepted(submission: &Submission, keychain_profile: &str) -> Result<()> {
    let submission_id = submission.id.as_deref().unwrap_or("<unknown>");

    match submission.status.as_deref() {
        Some(ACCEPTED_STATUS) => Ok(()),
        Some(status) => Err(anyhow!(
            "Apple rejected the notarization submission {submission_id} with status `{status}`; \
             run `xcrun notarytool log {submission_id} --keychain-profile {keychain_profile}` to see why"
        )),
        None => Err(anyhow!(
            "notarization submission {submission_id} did not report a status ({}), so it is not \
             accepted; run `xcrun notarytool log {submission_id} --keychain-profile {keychain_profile}` to see why",
            submission
                .message
                .as_deref()
                .unwrap_or("no message from notarytool")
        )),
    }
}

/// Submit `dmg_path` to Apple's notary service and staple the ticket.
///
/// Notarization is required for distribution of signed apps outside the Mac App
/// Store to avoid Gatekeeper warnings on macOS 10.15+.
pub fn notarize_dmg(
    keychain_profile: &str,
    dmg_path: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!(
        keychain_profile = %keychain_profile,
        dmg_path = %dmg_path.display(),
        "Submitting DMG for notarization"
    );

    let args = submit_arguments(keychain_profile, dmg_path, command_output_mode);
    let output = execute_command_capture_stdout("xcrun", args, None, None::<&[(&str, &str)]>)
        .with_context(|| format!("failed to submit `{}` for notarization", dmg_path.display()))?;

    let submission = parse_submission(&output)?;
    ensure_accepted(&submission, keychain_profile)?;
    info!(
        submission_id = submission.id.as_deref().unwrap_or("<unknown>"),
        "Notarization accepted"
    );

    debug!("Stapling notarization ticket for offline verification");
    let mut staple_args = command_args!(capacity = 4, "stapler", "staple", dmg_path);
    if command_output_mode == &CommandOutputMode::Verbose {
        command_args_push!(staple_args, "--verbose");
    }
    execute_command(
        "xcrun",
        staple_args,
        None,
        None::<&[(&str, &str)]>,
        command_output_mode,
    )?;

    info!("DMG notarization and stapling completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "AC_PASSWORD";

    fn submission(json: &str) -> Submission {
        parse_submission(json).expect("the fixture must parse")
    }

    #[test]
    fn the_submission_asks_for_json_and_waits() {
        let args = submit_arguments(
            PROFILE,
            Path::new("/dist/Example.dmg"),
            &CommandOutputMode::Normal,
        );

        assert_eq!(
            args,
            command_args!(
                "notarytool",
                "submit",
                "/dist/Example.dmg",
                "--keychain-profile",
                PROFILE,
                "--wait",
                "--output-format",
                "json",
            )
        );
    }

    #[test]
    fn an_accepted_submission_passes() {
        let accepted = submission(
            r#"{"status":"Accepted","message":"Successfully received submission info","id":"2efe2717-52ef-43a5-96dc-0797e4ca1041","name":"Example.dmg","createdDate":"2026-10-22T03:00:00.000Z"}"#,
        );

        assert!(ensure_accepted(&accepted, PROFILE).is_ok());
    }

    /// `notarytool` exits 0 for this, which is exactly why the status is read.
    #[test]
    fn a_rejected_submission_names_its_id_and_the_log_command() {
        let rejected = submission(
            r#"{"status":"Invalid","message":"Processing failed","id":"b1c2d3e4-0000-1111-2222-333344445555"}"#,
        );

        let error = ensure_accepted(&rejected, PROFILE).unwrap_err().to_string();

        assert!(error.contains("Invalid"), "{error}");
        assert!(
            error.contains("b1c2d3e4-0000-1111-2222-333344445555"),
            "{error}"
        );
        assert!(
            error.contains("xcrun notarytool log b1c2d3e4-0000-1111-2222-333344445555"),
            "{error}"
        );
    }

    /// A submission that ran out of time reports no status at all.
    #[test]
    fn a_submission_without_a_status_is_not_accepted() {
        let timed_out = submission(
            r#"{"id":"b1c2d3e4-0000-1111-2222-333344445555","message":"Timeout of 1 second(s) was reached before processing completed."}"#,
        );

        let error = ensure_accepted(&timed_out, PROFILE)
            .unwrap_err()
            .to_string();

        assert!(error.contains("did not report a status"), "{error}");
        assert!(error.contains("Timeout of 1 second(s)"), "{error}");
    }

    #[test]
    fn output_that_is_not_json_is_reported_with_what_was_printed() {
        let error = parse_submission("notarytool: error: HTTP 403")
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("could not read the notarization result"),
            "{error}"
        );
        assert!(error.contains("HTTP 403"), "{error}");
    }
}

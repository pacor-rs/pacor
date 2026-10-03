//! Apple notarization and ticket stapling.

use std::path::Path;

use anyhow::Result;

use pacor_core::utils::command::{
    CommandOutputMode, execute_command,
    macros::{command_args, command_args_push},
};

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

    let mut submit_args = command_args!(
        capacity = 7,
        "notarytool",
        "submit",
        dmg_path,
        "--keychain-profile",
        keychain_profile,
        "--wait",
    );
    if command_output_mode == &CommandOutputMode::Verbose {
        command_args_push!(submit_args, "--verbose");
    }
    execute_command(
        "xcrun",
        submit_args,
        None,
        None::<&[(&str, &str)]>,
        command_output_mode,
    )?;

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

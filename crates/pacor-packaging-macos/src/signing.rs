//! Code signing for the produced DMG.

use std::path::Path;

use anyhow::Result;

use pacor_core::utils::command::{
    CommandOutputMode, execute_command,
    macros::{command_args, command_args_push},
};

/// Sign `dmg_path` with a `codesign` identity.
///
/// Signing is a prerequisite for notarization and for distribution outside the
/// Mac App Store.
pub fn sign_dmg(
    identity: &str,
    dmg_path: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!(
        signing_identity = %identity,
        dmg_path = %dmg_path.display(),
        "Signing DMG"
    );

    let mut args = command_args!(capacity = 4, "-s", identity, dmg_path);
    if command_output_mode == &CommandOutputMode::Verbose {
        command_args_push!(args, "--verbose");
    }

    execute_command(
        "codesign",
        args,
        None,
        None::<&[(&str, &str)]>,
        command_output_mode,
    )?;

    info!("DMG code signature applied");
    Ok(())
}

//! Flutter build invocation for the Linux target.

use std::path::Path;

use anyhow::Result;

use pacor_core::utils::command::{CommandOutputMode, macros::command_args};

/// Build the Flutter Linux application in release mode with obfuscation.
///
/// Debug symbols go to `tmp/` (relative to the project root) to keep the
/// shipped bundle small; [`crate::LinuxBuilder`] removes that directory when
/// cleaning.
pub(crate) fn build(project_root: &Path, command_output_mode: &CommandOutputMode) -> Result<()> {
    info!("Building Linux app in release mode...");

    let args = command_args!(
        "linux",
        "--release",
        "--obfuscate",
        "--split-debug-info=tmp/"
    );

    pacor_core::packaging::flutter_build(&args, project_root, command_output_mode, "linux")?;

    info!("Linux app built successfully");
    Ok(())
}

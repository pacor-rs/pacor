//! Flutter build invocation for the macOS target.

use std::path::Path;

use anyhow::Result;

use pacor_core::utils::command::{
    CommandOutputMode, execute_flutter_build_command, macros::command_args,
};

/// Build the Flutter macOS application in release mode with obfuscation.
///
/// Debug symbols are written to `tmp/` (relative to the project root) to keep
/// the shipped binary small; [`crate::MacOSAppBuilder`] removes that directory
/// during cleanup.
pub fn build_flutter_app(
    project_root: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!("Building Flutter macOS application in release mode with obfuscation");

    let build_args = command_args!(
        "macos",
        "--release",
        "--obfuscate",
        "--split-debug-info=tmp/"
    );

    execute_flutter_build_command(&build_args, project_root, command_output_mode)?;

    info!("Flutter macOS application built successfully");
    Ok(())
}

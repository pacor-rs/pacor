//! Flutter build invocation for the macOS target.

use std::path::Path;

use anyhow::Result;

use pacor_core::{
    config::FlutterBuildMode,
    utils::command::{CommandOutputMode, execute_flutter_build_command, macros::command_args},
};

/// Build the Flutter macOS application in `mode`.
///
/// Release and profile builds are obfuscated and write their debug symbols to
/// `tmp/` (relative to the project root) to keep the shipped binary small;
/// [`crate::MacOSAppBuilder`] removes that directory during cleanup. Debug
/// builds cannot be obfuscated, because obfuscation needs an AOT build.
pub fn build_flutter_app(
    project_root: &Path,
    mode: FlutterBuildMode,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!("Building Flutter macOS application ({mode})");

    let mode_flag = format!("--{}", mode.as_str());
    let mut build_args = command_args!(capacity = 4, "macos", mode_flag.as_str());

    if mode.supports_obfuscation() {
        build_args.push("--obfuscate".into());
        build_args.push("--split-debug-info=tmp/".into());
    } else {
        debug!("Not obfuscating: {mode} builds are not AOT");
    }

    execute_flutter_build_command(&build_args, project_root, command_output_mode)?;

    info!("Flutter macOS application built successfully");
    Ok(())
}

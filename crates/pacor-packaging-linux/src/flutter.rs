//! Flutter build invocation for the Linux target.

use std::path::Path;

use anyhow::Result;

use pacor_core::{
    config::FlutterBuildMode,
    utils::command::{CommandOutputMode, macros::command_args},
};

/// Build the Flutter Linux application in `mode`.
///
/// Release and profile builds are obfuscated and write their debug symbols to
/// `tmp/` (relative to the project root) to keep the shipped bundle small;
/// [`crate::LinuxBuilder`] removes that directory when cleaning. Debug builds
/// cannot be obfuscated, because obfuscation needs an AOT build.
pub(crate) fn build(
    project_root: &Path,
    mode: FlutterBuildMode,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!("Building Linux app ({mode})...");

    let mode_flag = format!("--{}", mode.as_str());
    let mut args = command_args!(capacity = 4, "linux", mode_flag.as_str());

    if mode.supports_obfuscation() {
        args.push("--obfuscate".into());
        args.push("--split-debug-info=tmp/".into());
    } else {
        debug!("Not obfuscating: {mode} builds are not AOT");
    }

    pacor_core::packaging::flutter_build(&args, project_root, command_output_mode, "linux")?;

    info!("Linux app built successfully");
    Ok(())
}

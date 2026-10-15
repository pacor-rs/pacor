//! `flutter build windows` invocation for the Windows target.

use std::path::Path;

use anyhow::Result;

use pacor_core::{
    config::FlutterBuildMode,
    utils::command::{CommandOutputMode, macros::command_args},
};

/// Directory (relative to the project root) that split debug symbols go to.
///
/// Keeping them out of the bundle is what `--split-debug-info` is for; they stay
/// next to the project until [`crate::WindowsBuilder::clean`] removes them.
pub(crate) const SYMBOLS_DIR: &str = "tmp";

/// Builds the Flutter application for Windows in `mode`.
///
/// Release and profile builds are obfuscated and split their debug symbols into
/// [`SYMBOLS_DIR`]. Debug builds cannot be obfuscated — obfuscation needs an AOT
/// build — so the flags are omitted there instead of letting Flutter reject the
/// command.
pub(crate) fn build(
    project_root: &Path,
    mode: FlutterBuildMode,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    info!("Building Flutter application for Windows ({mode})");

    let mode_flag = format!("--{}", mode.as_str());
    let mut args = command_args!(capacity = 4, "windows", mode_flag.as_str());

    if mode.supports_obfuscation() {
        args.push("--obfuscate".into());
        args.push(format!("--split-debug-info={SYMBOLS_DIR}/").into());
    } else {
        debug!("Not obfuscating: {mode} builds are not AOT");
    }

    pacor_core::packaging::flutter_build(&args, project_root, command_output_mode, "windows")?;

    info!("Windows application built successfully");
    Ok(())
}

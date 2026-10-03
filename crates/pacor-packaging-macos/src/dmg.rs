//! DMG installer creation via the bundled `create-dmg` script.

use std::{fs, path::Path, path::PathBuf};

use anyhow::Result;

use pacor_core::utils::command::{
    CommandOutputMode, execute_command,
    macros::{command_args, command_args_extend},
};
use pacor_core::utils::fs::copy_directory_with_parent;

/// Everything the DMG builder needs from the surrounding configuration.
pub struct DmgOptions<'a> {
    /// Path to the `create-dmg` shell script.
    pub create_dmg_script: &'a Path,
    /// Working directory for the script.
    pub project_root: &'a Path,
    /// Directory the `.dmg` is written to.
    pub output_dir: &'a Path,
    /// Application name, used for the staging directory and volume name.
    pub app_name: &'a str,
    /// Optional background image for the installer window.
    pub background: Option<&'a str>,
    /// Optional installer window size.
    pub window_size: Option<(u32, u32)>,
}

/// Create the `.dmg` installer for `app_bundle_path`.
///
/// The `.app` bundle is copied into a temporary staging directory so the build
/// output itself is never modified.
pub fn create_dmg_installer(
    options: &DmgOptions<'_>,
    app_bundle_path: &Path,
    filename_base: &str,
    command_output_mode: &CommandOutputMode,
) -> Result<PathBuf> {
    let dmg_path = options.output_dir.join(format!("{filename_base}.dmg"));
    info!(dmg_path = %dmg_path.display(), "Creating DMG installer");

    if dmg_path.exists() {
        debug!(existing_dmg = %dmg_path.display(), "Removing existing DMG");
        fs::remove_file(&dmg_path)?;
    }

    // The guard must outlive the create-dmg invocation.
    let staging_guard = tempfile::TempDir::with_prefix("pacor_dmg_")?;
    let staging_dir = staging_guard.path().join(options.app_name);
    copy_directory_with_parent(app_bundle_path, &staging_dir)?;
    trace!(staging_dir = %staging_dir.display(), "App bundle staged for DMG");

    let volume_name = format!("{} Installer", options.app_name);
    let mut dmg_args = command_args!(
        capacity = 14,
        options.create_dmg_script,
        "--volname",
        volume_name,
        "--sandbox-safe",
        "--app-drop-link",
        "480",
        "120",
    );

    if let Some(background) = options.background {
        command_args_extend!(dmg_args, "--background", background);
    }
    if let Some((width, height)) = options.window_size {
        command_args_extend!(dmg_args, "--window-size", width, height);
    }
    command_args_extend!(dmg_args, dmg_path, staging_dir);

    execute_command(
        "bash",
        dmg_args,
        Some(options.project_root),
        None::<&[(&str, &str)]>,
        command_output_mode,
    )?;

    info!(dmg_path = %dmg_path.display(), "DMG installer created");
    Ok(dmg_path)
}

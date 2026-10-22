//! DMG installer creation via the bundled `create-dmg` script.

use std::{ffi::OsString, fs, path::Path, path::PathBuf};

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

/// The argument vector for the embedded `create-dmg` script.
///
/// `--sandbox-safe` makes the script skip the Finder AppleScript that lays the installer
/// window out ("Skipping Finder-prettifying AppleScript because we are in Sandbox..."), and
/// that AppleScript is the only consumer of `--background` and `--window-size`. Passing it
/// unconditionally made both settings dead configuration, so it is only passed when no layout
/// was asked for.
fn dmg_arguments(options: &DmgOptions<'_>, dmg_path: &Path, staging_dir: &Path) -> Vec<OsString> {
    let volume_name = format!("{} Installer", options.app_name);
    let mut args = command_args!(
        capacity = 14,
        options.create_dmg_script,
        "--volname",
        volume_name,
    );

    if options.background.is_none() && options.window_size.is_none() {
        command_args_extend!(args, ["--sandbox-safe"]);
    }
    command_args_extend!(args, "--app-drop-link", "480", "120");
    if let Some(background) = options.background {
        command_args_extend!(args, "--background", background);
    }
    if let Some((width, height)) = options.window_size {
        command_args_extend!(args, "--window-size", width, height);
    }
    command_args_extend!(args, dmg_path, staging_dir);

    args
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

    let dmg_args = dmg_arguments(options, &dmg_path, &staging_dir);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn options(background: Option<&str>, window_size: Option<(u32, u32)>) -> DmgOptions<'_> {
        DmgOptions {
            create_dmg_script: Path::new("/cache/pacor/macos/create-dmg.sh"),
            project_root: Path::new("/project"),
            output_dir: Path::new("/project/dist"),
            app_name: "Demo",
            background,
            window_size,
        }
    }

    fn args_of(options: &DmgOptions<'_>) -> Vec<String> {
        dmg_arguments(
            options,
            Path::new("/project/dist/Demo-1.0.0-x86_64.dmg"),
            Path::new("/tmp/pacor_dmg_123/Demo"),
        )
        .iter()
        .map(|argument| argument.to_string_lossy().to_string())
        .collect()
    }

    #[test]
    fn a_plain_installer_runs_the_script_sandbox_safe() {
        assert_eq!(
            args_of(&options(None, None)),
            [
                "/cache/pacor/macos/create-dmg.sh",
                "--volname",
                "Demo Installer",
                "--sandbox-safe",
                "--app-drop-link",
                "480",
                "120",
                "/project/dist/Demo-1.0.0-x86_64.dmg",
                "/tmp/pacor_dmg_123/Demo",
            ]
        );
    }

    #[test]
    fn a_configured_background_drops_the_sandbox_safe_flag() {
        let args = args_of(&options(Some("/project/assets/background.png"), None));

        assert!(
            !args.iter().any(|argument| argument == "--sandbox-safe"),
            "the AppleScript that draws the background is skipped under --sandbox-safe: {args:?}"
        );
        let background = args
            .iter()
            .position(|argument| argument == "--background")
            .unwrap();
        assert_eq!(args[background + 1], "/project/assets/background.png");
    }

    #[test]
    fn a_configured_window_size_drops_the_sandbox_safe_flag() {
        let args = args_of(&options(None, Some((500, 350))));

        assert!(
            !args.iter().any(|argument| argument == "--sandbox-safe"),
            "the AppleScript that sizes the window is skipped under --sandbox-safe: {args:?}"
        );
        let window_size = args
            .iter()
            .position(|argument| argument == "--window-size")
            .unwrap();
        assert_eq!(&args[window_size + 1..window_size + 3], ["500", "350"]);
    }
}

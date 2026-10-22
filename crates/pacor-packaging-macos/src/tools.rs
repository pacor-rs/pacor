//! Provisioning of the macOS packaging tools and templates.
//!
//! The DMG creation script and its AppleScript/EULA support files are embedded in the binary
//! and written under the user's config directory on every run, so no extra download is
//! required and a cached copy can never be older than the binary that is running: the argument
//! vector pacor hands `create-dmg.sh` and the script's own version have to match.

use std::path::Path;

use anyhow::Result;

use pacor_core::utils::fs::{
    ensure_directory_exists, write_executable_file, write_file_atomically,
};

const EULA_RESOURCES_TEMPLATE_XML: &str =
    include_str!("../../../tools/macos/support/eula-resources-template.xml");
const DMG_APPLESCRIPT_TEMPLATE: &str =
    include_str!("../../../tools/macos/support/template.applescript");
const CREATE_DMG_SHELL_SCRIPT: &str = include_str!("../../../tools/macos/create-dmg.sh");

/// Write the embedded tools and templates.
///
/// `create_dmg_script_path` is expected to live inside `tools_dir`.
pub fn prepare_packaging_tools(tools_dir: &Path, create_dmg_script_path: &Path) -> Result<()> {
    debug!(directory = %tools_dir.display(), "Preparing macOS packaging tools");

    ensure_directory_exists(tools_dir)?;

    let support_dir = tools_dir.join("support");
    ensure_directory_exists(&support_dir)?;

    write_file_atomically(
        &support_dir.join("eula-resources-template.xml"),
        EULA_RESOURCES_TEMPLATE_XML.as_bytes(),
    )?;
    write_file_atomically(
        &support_dir.join("template.applescript"),
        DMG_APPLESCRIPT_TEMPLATE.as_bytes(),
    )?;

    // The script must be executable.
    trace!(path = %create_dmg_script_path.display(), "Installing create-dmg script");
    write_executable_file(create_dmg_script_path, CREATE_DMG_SHELL_SCRIPT.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// A cached tool is refreshed on every run, so an upgraded pacor cannot drive a
    /// `create-dmg.sh` older than the argument vector this binary hands it.
    #[test]
    fn a_stale_cached_tool_is_replaced_by_the_embedded_one() {
        let temp = TempDir::new().unwrap();
        let tools_dir = temp.path().join("macos");
        let script = tools_dir.join("create-dmg.sh");
        let support_dir = tools_dir.join("support");
        fs::create_dir_all(&support_dir).unwrap();
        fs::write(&script, b"#!/bin/sh\n# stale\n").unwrap();
        fs::write(support_dir.join("template.applescript"), b"stale").unwrap();

        prepare_packaging_tools(&tools_dir, &script).unwrap();

        assert_eq!(
            fs::read(&script).unwrap(),
            CREATE_DMG_SHELL_SCRIPT.as_bytes()
        );
        assert_eq!(
            fs::read(support_dir.join("template.applescript")).unwrap(),
            DMG_APPLESCRIPT_TEMPLATE.as_bytes()
        );
    }

    /// The script is invoked through `bash`, so the refresh must not drop the exec bit.
    #[cfg(unix)]
    #[test]
    fn the_refreshed_script_stays_executable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let tools_dir = temp.path().join("macos");
        let script = tools_dir.join("create-dmg.sh");
        fs::create_dir_all(&tools_dir).unwrap();
        fs::write(&script, b"stale").unwrap();

        prepare_packaging_tools(&tools_dir, &script).unwrap();

        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "mode {mode:o}");
    }
}

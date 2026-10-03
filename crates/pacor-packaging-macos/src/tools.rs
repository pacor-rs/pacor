//! Provisioning of the macOS packaging tools and templates.
//!
//! The DMG creation script and its AppleScript/EULA support files are embedded
//! in the binary and materialised under the user's config directory on first
//! use, so no extra download is required.

use std::{fs, path::Path};

use anyhow::Result;

use pacor_core::utils::fs::{ensure_directory_exists, write_executable_file};

const EULA_RESOURCES_TEMPLATE_XML: &str =
    include_str!("../../../tools/macos/support/eula-resources-template.xml");
const DMG_APPLESCRIPT_TEMPLATE: &str =
    include_str!("../../../tools/macos/support/template.applescript");
const CREATE_DMG_SHELL_SCRIPT: &str = include_str!("../../../tools/macos/create-dmg.sh");

/// Write the embedded tools/templates if they are not present yet.
///
/// `create_dmg_script_path` is expected to live inside `tools_dir`.
pub fn prepare_packaging_tools(tools_dir: &Path, create_dmg_script_path: &Path) -> Result<()> {
    debug!(directory = %tools_dir.display(), "Preparing macOS packaging tools");

    ensure_directory_exists(tools_dir)?;

    let support_dir = tools_dir.join("support");
    ensure_directory_exists(&support_dir)?;

    write_if_absent(
        &support_dir.join("eula-resources-template.xml"),
        EULA_RESOURCES_TEMPLATE_XML.as_bytes(),
    )?;
    write_if_absent(
        &support_dir.join("template.applescript"),
        DMG_APPLESCRIPT_TEMPLATE.as_bytes(),
    )?;

    // The script must be executable.
    if !create_dmg_script_path.exists() {
        trace!(path = %create_dmg_script_path.display(), "Installing create-dmg script");
        write_executable_file(create_dmg_script_path, CREATE_DMG_SHELL_SCRIPT.as_bytes())?;
    }

    Ok(())
}

fn write_if_absent(path: &Path, contents: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    trace!(path = %path.display(), "Installing template");
    fs::write(path, contents)?;
    Ok(())
}

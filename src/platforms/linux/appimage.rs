//! AppImage bundle creation for Linux platforms.
//!
//! This module provides functionality to create AppImage bundles, which are portable
//! application formats for Linux. AppImages contain all dependencies and can run
//! on most Linux distributions without installation.
//!
//! The AppImage format follows the AppDir structure with:
//! - AppRun launcher script
//! - Desktop entry file
//! - Application icon
//! - usr/ directory containing the application and its dependencies
//!
//! The build process involves:
//! 1. Downloading and applying template files
//! 2. Creating the AppDir structure
//! 3. Populating with application files and dependencies
//! 4. Running linuxdeploy to generate the final AppImage

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::args::CommandOutputMode;
use crate::core::arch::Arch;
use crate::core::freedesktop::FreeDesktop;
use crate::error::{BundleError, Result};
use crate::platforms::linux::icon::{Icon, copy_icon_to_dir};
use crate::utils::command::{
    execute_command,
    macros::{command_args, command_args_extend},
};
use crate::utils::downloader::download_file;
use crate::utils::fs::{
    clean_dir, copy_dir, create_symlink, ensure_directory_exists, write_executable_file,
};
use crate::utils::template::render_template;

/// Configuration parameters required to build an AppImage bundle.
///
/// This struct contains all the necessary information and paths needed
/// to construct a complete AppImage, including application metadata,
/// file paths, and platform-specific configuration.
pub(super) struct AppImageBuildConfig<'a> {
    /// Display name of the application used in metadata
    pub(super) app_display_name: &'a str,
    /// Name of the application executable file
    pub(super) app_executable_name: &'a str,
    /// Primary application icon used as the main AppImage icon
    pub(super) primary_icon: &'a Icon,
    /// Collection of additional application icons indexed by size
    pub(super) additional_icons: &'a BTreeMap<u32, Icon>,
    /// Path to the Flutter build output directory
    pub(super) flutter_build_path: &'a Path,
    /// Root directory of the project
    pub(super) project_root: &'a Path,
    /// Directory containing build tools and templates
    pub(super) tools_directory: &'a Path,
    /// Directory where the final AppImage will be output
    pub(super) output_directory: &'a Path,
    /// FreeDesktop
    pub(super) desktop: &'a FreeDesktop,
    /// Target CPU architecture for the AppImage
    pub(super) target_architecture: &'a Arch,
    /// Application version string
    pub(super) version: &'a str,
}

/// Builder responsible for creating AppImage bundles.
///
/// This builder handles the complete process of creating an AppImage,
/// including downloading templates, assembling application data, generating
/// the AppDir structure, and creating the final AppImage bundle.
pub(super) struct AppImageBuilder<'a> {
    app_display_name: &'a str,
    app_executable_name: &'a str,
    primary_icon: &'a Icon,
    additional_icons: &'a BTreeMap<u32, Icon>,
    flutter_build_path: &'a Path,
    tools_directory: &'a Path,
    project_root: &'a Path,
    output_directory: &'a Path,
    desktop: &'a FreeDesktop,
    target_architecture: &'a Arch,
    version: &'a str,
}

/// Template file information for downloading and processing
struct AppImageTemplate<'a> {
    /// Remote filename on the template repository
    remote_filename: &'a str,
    /// Local path where the template should be applied
    local_file_path: &'a Path,
}

impl<'a> AppImageBuilder<'a> {
    /// Creates a new AppImage builder from the provided configuration.
    ///
    /// # Arguments
    /// * `config` - Build configuration containing all necessary parameters
    ///
    /// # Returns
    /// A new instance of `AppImageBuilder` ready to build AppImages
    pub(super) fn from_config(config: AppImageBuildConfig<'a>) -> Self {
        debug!(
            "Initializing AppImage builder for '{}'",
            config.app_executable_name
        );

        Self {
            app_executable_name: config.app_executable_name,
            app_display_name: config.app_display_name,
            primary_icon: config.primary_icon,
            additional_icons: config.additional_icons,
            flutter_build_path: config.flutter_build_path,
            tools_directory: config.tools_directory,
            output_directory: config.output_directory,
            desktop: config.desktop,
            target_architecture: config.target_architecture,
            version: config.version,
            project_root: config.project_root,
        }
    }

    /// Downloads and applies necessary template files for the build process.
    ///
    /// This method downloads Flutter/Linux template files from the remote repository
    /// and replaces placeholders with actual application values. It creates backups
    /// of original files that can be restored later.
    ///
    /// # Returns
    /// A vector of file pairs [modified_file, backup_file] for later restoration
    pub(super) fn download_and_apply_templates(&self) -> Result<Vec<[PathBuf; 2]>> {
        info!("Downloading and applying build templates for AppImage");

        let templates = [AppImageTemplate {
            remote_filename: "my_application_template.cc",
            local_file_path: &self
                .project_root
                .join("linux")
                .join("runner")
                .join("my_application.cc"),
        }];

        let mut modified_files = Vec::with_capacity(templates.len());

        for template in &templates {
            let file_pair = self
                .download_and_apply_template(template.remote_filename, template.local_file_path)?;
            modified_files.push(file_pair);
        }

        info!(
            "Successfully applied {} build templates",
            modified_files.len()
        );
        Ok(modified_files)
    }

    /// Downloads a single template file and applies it to the local project.
    ///
    /// This method handles downloading template files from the remote repository,
    /// caching them locally, replacing placeholders, and creating backups.
    ///
    /// # Arguments
    /// * `remote_filename` - Name of the template file in the remote repository
    /// * `local_file_path` - Path to the local file that will be modified
    ///
    /// # Returns
    /// A tuple of [modified_file_path, backup_file_path]
    fn download_and_apply_template(
        &self,
        remote_filename: &str,
        local_file_path: &Path,
    ) -> Result<[PathBuf; 2]> {
        debug!("Processing template: {}", remote_filename);

        // Generate local filenames and paths
        let local_filename = remote_filename.replace("_template", "");
        let backup_file_path = local_file_path.with_file_name(format!("{}.bak", local_filename));

        let appimage_tools_dir = self.tools_directory.join("appimage");
        let local_template_path = appimage_tools_dir.join(remote_filename);
        ensure_directory_exists(&appimage_tools_dir)?;

        // Download or read cached template data
        let template_data = if !local_template_path.exists() {
            debug!("Template not cached, downloading from remote repository");

            let template_url = format!(
                "https://raw.githubusercontent.com/pacor-rs/tools/refs/heads/main/linux/appimage/{}",
                remote_filename
            );

            let template_data = download_file(&template_url)?;
            fs::write(&local_template_path, &template_data)?;

            info!(
                "Template downloaded and cached: {}",
                local_template_path.display()
            );

            template_data
        } else {
            debug!("Using cached template: {}", local_template_path.display());
            fs::read(&local_template_path)?
        };

        // Process template content and apply substitutions
        let template_content = String::from_utf8_lossy(&template_data);
        let replacements = HashMap::from([
            ("APP_EXECUTABLE_NAME", self.app_executable_name),
            // ("APP_DISPLAY_NAME", self.app_display_name), // TODO: Add support for display name later
        ]);
        let processed_content = render_template(&template_content, &replacements);

        // Create backup and apply template
        fs::rename(local_file_path, &backup_file_path)?;
        fs::write(local_file_path, &processed_content)?;

        info!(
            "Template applied to project file: {}",
            local_file_path.display()
        );

        Ok([local_file_path.to_path_buf(), backup_file_path])
    }

    /// Restores original project files from their backups.
    ///
    /// This method should be called after the build process is complete
    /// to restore the original state of modified project files.
    ///
    /// # Arguments
    /// * `modified_files` - Vector of [file, backup] pairs to restore
    pub(super) fn restore_original_files(&self, modified_files: &[[PathBuf; 2]]) -> Result<()> {
        debug!("Restoring {} original project files", modified_files.len());

        for [original_file, backup_file] in modified_files {
            fs::rename(backup_file, original_file)?;
            debug!("Restored file: {}", original_file.display());
        }

        info!("All original project files restored successfully");

        Ok(())
    }

    /// Downloads and prepares the linuxdeploy tool required for AppImage creation.
    ///
    /// This method downloads the architecture-specific linuxdeploy tool and patches
    /// its magic bytes to prevent it from being treated as an AppImage by desktop
    /// environments during the build process.
    fn prepare_linuxdeploy_tool(&self, command_output_mode: &CommandOutputMode) -> Result<PathBuf> {
        let appimage_tool_dir = self.tools_directory.join("appimage");
        ensure_directory_exists(&appimage_tool_dir)?;

        let linuxdeploy_arch = match self.target_architecture {
            Arch::X64 => "x86_64",
            Arch::ARM64 => "aarch64",
        };

        let linuxdeploy_path =
            appimage_tool_dir.join(format!("linuxdeploy-{linuxdeploy_arch}.AppImage"));

        if !linuxdeploy_path.exists() {
            let url = format!(
                "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-{linuxdeploy_arch}.AppImage"
            );
            debug!(url = %url, "Downloading linuxdeploy tool");
            let data = download_file(&url)?;
            write_executable_file(&linuxdeploy_path, &data)?;

            info!(
                "Linuxdeploy tool downloaded: {}",
                linuxdeploy_path.display()
            );
        }

        // Patch the AppImage magic bytes to avoid being treated as AppImage by desktop environments
        debug!(tool_path = %linuxdeploy_path.display(), "Patching linuxdeploy AppImage magic bytes");
        let dd_args = [
            "if=/dev/zero",
            "bs=1",
            "count=3",
            "seek=8",
            "conv=notrunc",
            &format!("of={}", linuxdeploy_path.display()),
        ];
        execute_command(
            "dd",
            dd_args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )?;

        Ok(linuxdeploy_path)
    }

    /// Builds the complete AppImage and returns the path to the resulting file.
    ///
    /// This is the main entry point for the build process, orchestrating all
    /// the necessary steps to create a valid AppImage bundle.
    ///
    /// # Arguments
    /// * `command_output_mode` - Specifies the level of command output to display.
    ///
    /// # Returns
    /// Path to the generated AppImage file
    pub(super) fn build_appimage(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<PathBuf> {
        let appimage_filename = format!(
            "{}_{}_{}.AppImage",
            self.app_executable_name, self.version, self.target_architecture
        );
        let expected_appimage_path = self.output_directory.join(&appimage_filename);

        info!(
            "Starting AppImage build: {} -> {}",
            appimage_filename,
            expected_appimage_path.display()
        );

        // Prepare build tools
        let linuxdeploy_path = self.prepare_linuxdeploy_tool(command_output_mode)?;
        debug!(linuxdeploy_path = %linuxdeploy_path.display(), "Linuxdeploy tool ready");

        // Create and populate AppDir structure
        let app_dir = self.create_app_dir()?;
        self.populate_app_dir(&app_dir)?;
        self.create_apprun_script(&app_dir)?;
        self.copy_icons_to_app_dir(&app_dir)?;
        self.create_root_icon_symlink(&app_dir)?;

        // Generate AppImage using linuxdeploy
        self.run_linuxdeploy(&linuxdeploy_path, &app_dir, command_output_mode)?;

        // Finalize AppImage
        let final_appimage_path = self.locate_and_rename_appimage(&expected_appimage_path)?;
        self.ensure_appimage_executable(&final_appimage_path)?;

        info!(
            path = %final_appimage_path.display(),
            "AppImage build completed successfully",
        );

        Ok(final_appimage_path)
    }

    /// Creates and returns the path to the AppDir.
    ///
    /// The AppDir is the staging directory that contains all files and
    /// directories that will be included in the final AppImage.
    ///
    /// # Returns
    /// Path to the created AppDir
    fn create_app_dir(&self) -> Result<PathBuf> {
        let temp_dir = tempfile::TempDir::with_prefix("pacor_appimage_")?;
        let app_dir = temp_dir
            .path()
            .join(format!("{}.AppDir", self.app_executable_name));
        clean_dir(&app_dir)?;
        ensure_directory_exists(&app_dir)?;
        debug!(app_dir = %app_dir.display(), "AppDir created");
        Ok(app_dir)
    }

    /// Copies the Flutter build output into the AppDir structure
    fn populate_app_dir(&self, app_dir: &Path) -> Result<()> {
        let app_usr_dir = app_dir.join("usr");
        copy_dir(self.flutter_build_path, &app_usr_dir)?;

        let bin_dir = app_dir.join("usr").join("bin");
        ensure_directory_exists(&bin_dir)?;
        fs::rename(
            app_usr_dir.join(self.app_executable_name),
            bin_dir.join(self.app_executable_name),
        )?;
        debug!(
            executable = %self.app_executable_name,
            from = %app_usr_dir.display(),
            to = %bin_dir.display(),
            "Executable moved to bin directory"
        );
        Ok(())
    }

    /// Creates the AppRun launcher script in the AppDir root.
    ///
    /// The AppRun script is the entry point for the AppImage. It sets up
    /// the runtime environment and launches the main application executable.
    ///
    /// # Arguments
    /// * `app_dir` - Root directory of the AppDir structure
    fn create_apprun_script(&self, app_dir: &Path) -> Result<()> {
        let apprun_content = format!(
            r#"#!/bin/sh
cd "$(dirname "$0")"
exec ./usr/bin/{} "$@"
"#,
            self.app_executable_name
        );
        let apprun_path = app_dir.join("AppRun");
        write_executable_file(&apprun_path, apprun_content.as_bytes())?;
        debug!(apprun_path = %apprun_path.display(), "AppRun script created");
        Ok(())
    }

    /// Copies all icons to the standard hicolor directory inside AppDir.
    ///
    /// This method installs application icons into the hicolor icon theme directory
    /// structure following the XDG Icon Theme Specification for proper system integration.
    ///
    /// # Arguments
    /// * `app_dir` - Root directory of the AppDir structure
    fn copy_icons_to_app_dir(&self, app_dir: &Path) -> Result<()> {
        let icons_dir = app_dir
            .join("usr")
            .join("share")
            .join("icons")
            .join("hicolor");
        ensure_directory_exists(&icons_dir)?;
        copy_icon_to_dir(
            self.app_executable_name,
            &icons_dir,
            self.additional_icons.values(),
        )?;
        debug!(icons_dir = %icons_dir.display(), "Icons copied to hicolor directory");
        Ok(())
    }

    /// Creates a symlink from the primary icon to the AppDir root (required by linuxdeploy).
    ///
    /// Linuxdeploy requires the application icon to be available at the AppDir root
    /// with the same name as the application. This creates a symbolic link to the
    /// primary icon in the hicolor theme directory.
    ///
    /// # Arguments
    /// * `app_dir` - Root directory of the AppDir structure
    fn create_root_icon_symlink(&self, app_dir: &Path) -> Result<()> {
        let target_icon = app_dir.join(format!(
            "{}.{}",
            self.app_executable_name,
            self.primary_icon.format()
        ));
        let source_icon = app_dir
            .join("usr")
            .join("share")
            .join("icons")
            .join("hicolor")
            .join(format!(
                "{}x{}",
                self.primary_icon.size(),
                self.primary_icon.size()
            ))
            .join(format!(
                "{}.{}",
                self.app_executable_name,
                self.primary_icon.format()
            ));

        if let Err(e) = create_symlink(&source_icon, &target_icon) {
            error!(
                source = %source_icon.display(),
                target = %target_icon.display(),
                error = %e,
                "Failed to create icon symlink"
            );
            return Err(BundleError::Build(format!(
                "Failed to create icon symlink: {e}"
            )));
        }
        debug!(source = %source_icon.display(), target = %target_icon.display(), "Icon symlink created");
        Ok(())
    }

    /// Executes linuxdeploy to generate the AppImage
    fn run_linuxdeploy(
        &self,
        linuxdeploy_path: &Path,
        app_dir: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        let bin_dir = app_dir.join("usr").join("bin");
        let executable_path = bin_dir.join(self.app_executable_name);
        let lib_dir = app_dir.join("usr").join("lib");

        let desktop_file = app_dir.join(format!("{}.desktop", self.app_executable_name));
        self.desktop.write(&desktop_file)?;

        let mut args = command_args!(
            capacity = 10,
            "--appdir",
            app_dir,
            "--executable",
            executable_path,
            "--desktop-file",
            desktop_file,
            "--output",
            "appimage",
        );

        if command_output_mode == &CommandOutputMode::Verbose {
            command_args_extend!(args, "--verbosity", "0"); // 0=debug, 1=info(default)
        }

        debug!(working_dir = %self.output_directory.display(), "Executing linuxdeploy");
        execute_command(
            linuxdeploy_path,
            args,
            Some(self.output_directory),
            Some(&[("LD_LIBRARY_PATH", &lib_dir)]),
            command_output_mode,
        )?;
        Ok(())
    }

    /// Finds the generated AppImage and renames it to the expected filename if necessary.
    ///
    /// Linuxdeploy generates AppImage files with its own naming convention. This method
    /// locates the generated file and renames it to match the expected output filename.
    ///
    /// # Arguments
    /// * `expected_path` - The desired path and filename for the AppImage
    ///
    /// # Returns
    /// Path to the final AppImage file
    fn locate_and_rename_appimage(&self, expected_path: &Path) -> Result<PathBuf> {
        let generated_appimage =
            find_generated_appimage(self.output_directory, self.app_display_name)?;
        if generated_appimage != expected_path {
            fs::rename(&generated_appimage, expected_path)?;
            debug!(
                from = %generated_appimage.display(),
                to = %expected_path.display(),
                "AppImage renamed to expected filename"
            );
        }
        Ok(expected_path.to_path_buf())
    }

    /// Ensures the final AppImage file has executable permissions.
    ///
    /// Sets executable permissions (0o755) on the final AppImage file to ensure
    /// it can be executed by the user and system.
    ///
    /// # Arguments
    /// * `appimage_path` - Path to the AppImage file
    fn ensure_appimage_executable(&self, appimage_path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(appimage_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(appimage_path, perms)?;
            debug!(path = %appimage_path.display(), "Set executable permissions on AppImage");
        }
        Ok(())
    }
}

/// Finds the generated AppImage file in the specified directory.
///
/// Linuxdeploy typically generates AppImage files with names like:
/// - AppName-<version>-<arch>.AppImage
/// - AppName-<arch>.AppImage
///
/// This function searches for files that start with the app name and have .AppImage extension.
///
/// # Arguments
/// * `dir` - Directory to search in
/// * `app_name` - Application name to match
///
/// # Returns
/// Path to the found AppImage file
fn find_generated_appimage(dir: &Path, app_name: &str) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "AppImage")
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(app_name))
        {
            return Ok(path);
        }
    }
    Err(BundleError::Build(format!(
        "Generated AppImage not found in {:?}",
        dir
    )))
}

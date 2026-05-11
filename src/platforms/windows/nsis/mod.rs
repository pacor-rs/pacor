use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{
    cli::args::CommandOutputMode,
    config::AppConfig,
    core::{
        arch::Arch,
        common::{ArtifactManager, BuildEnvironmentValidator, CommonBuilderImpl},
        project::FlutterProject,
    },
    error::{BuildError, PlatformError},
    platforms::{BuildArtifact, windows::nsis::generator::NSISGenerator},
    utils::{
        archive::extract_zip,
        command::{execute_command, find_command_path, macros::command_args},
        downloader::download_file,
        fs::ensure_directory_exists,
    },
};

mod generator;

/// URL for downloading NSIS (Nullsoft Scriptable Install System)
const NSIS_DOWNLOAD_URL: &str =
    "https://github.com/tauri-apps/binary-releases/releases/download/nsis-3/nsis-3.zip";

pub(super) struct NsisBuilder<'a> {
    /// Common builder functionality
    common: CommonBuilderImpl,
    app_config: &'a AppConfig,
    project_root: &'a Path,
    flutter_project: &'a FlutterProject,
    nsis_install_dir: &'a Path,
    executable_name: &'a str,
    default_legal_copyright: &'a str,
    icon_path: &'a Path,
}

impl<'a> NsisBuilder<'a> {
    pub(super) fn new(
        app_config: &'a AppConfig,
        project_root: &'a Path,
        flutter_project: &'a FlutterProject,
        nsis_install_dir: &'a Path,
        executable_name: &'a str,
        icon_path: &'a Path,
        default_legal_copyright: &'a str,
    ) -> Self {
        Self {
            common: CommonBuilderImpl,
            app_config,
            project_root,
            flutter_project,
            nsis_install_dir,
            executable_name,
            icon_path,
            default_legal_copyright,
        }
    }

    /// Downloads and extracts NSIS if not already present.
    fn ensure_nsis_installed(&self) -> Result<()> {
        let marker_file = self.nsis_install_dir.join(".extracted");

        if marker_file.exists() {
            debug!(nsis_dir = %self.nsis_install_dir.display(), "NSIS already installed; skipping download");
            return Ok(());
        }

        let archive_bytes = download_file(NSIS_DOWNLOAD_URL)?;
        ensure_directory_exists(self.nsis_install_dir)?;
        trace!(nsis_dir = %self.nsis_install_dir.display(), "Created NSIS install directory");

        trace!("Extracting NSIS to {}", self.nsis_install_dir.display());
        extract_zip(&archive_bytes, self.nsis_install_dir)?;

        fs::write(&marker_file, "")?; // Mark as extracted

        info!(path = %self.nsis_install_dir.display(), "NSIS installed successfully");
        Ok(())
    }

    /// Locates the `makensis.exe` executable, downloading NSIS if necessary.
    fn locate_makensis_executable(&self) -> Result<PathBuf> {
        if let Some(path) = find_command_path("makensis")? {
            info!(path = %path.display(), "Using system-installed makensis");
            return Ok(path);
        }

        let makensis_path = self.nsis_install_dir.join("makensis.exe");
        if !makensis_path.exists() {
            info!("makensis not found in PATH; installing NSIS...");
            self.ensure_nsis_installed()?;
            if !makensis_path.exists() {
                return Err(PlatformError::ToolNotFound {
                    tool: "makensis".to_string(),
                    platform: "Windows".to_string(),
                }
                .into());
            }
        }

        info!(path = %makensis_path.display(), "Using bundled makensis");
        Ok(makensis_path)
    }

    pub(super) fn build(
        &self,
        project_name: &str,
        version: &str,
        target_arch: &Arch,
        build_output_dir: &Path,
        installer_output_dir: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<BuildArtifact> {
        // Use common validation functionality
        self.validate_directory_permissions(installer_output_dir, "NSIS installer output")?;

        info!("Locating makensis executable...");
        let makensis_path = self.locate_makensis_executable()?;

        let setup_filename =
            self.generate_artifact_name(project_name, version, target_arch, "setup", "exe");

        let nsis_generator = NSISGenerator {
            app_config: self.app_config,
            default_description: self.flutter_project.description(),
            app_name: self.flutter_project.name(),
            version: self.flutter_project.version(),
            installer_name: &setup_filename,
            icon_path: self.icon_path,
            nsis_install_dir: self.nsis_install_dir,
            build_output_dir,
            installer_output_dir,
            executable_name: self.executable_name,
            default_legal_copyright: self.default_legal_copyright,
        };

        {
            let temp_dir = tempfile::TempDir::with_prefix("pacor-nsis-")?;

            let nsis_script_path = nsis_generator.generate(temp_dir.path())?;

            let nsis_args = command_args!["/INPUTCHARSET", "UTF8", "/V3", &nsis_script_path];

            info!("Running makensis to build installer...");
            execute_command(
                &makensis_path,
                &nsis_args,
                Some(self.project_root),
                None::<&[(&str, &str)]>,
                command_output_mode,
            )?;
        }

        let installer_path = installer_output_dir.join(&setup_filename);
        if !installer_path.exists() {
            error!(
                path = %installer_path.display(),
                "Installer binary not found after running makensis",
            );

            return Err(BuildError::ArtifactGenerationFailed {
                artifact: setup_filename,
                reason: "Installer binary not found after running makensis".to_string(),
            }
            .into());
        }

        debug!(path = %installer_path.display(), "Installer binary created");

        // Generate SHA1 checksum file using common functionality
        trace!("Generating SHA1 checksum file...");
        let artifact = self.create_artifact_with_checksum(installer_path)?;
        debug!(path = %artifact.sha1().display(), "SHA1 checksum file generated");

        info!(path = %artifact.file().display(), "NSIS installer built successfully");
        Ok(artifact)
    }

    /// Cleans up temporary files generated during build.
    pub(super) fn clean(&self) -> Result<()> {
        let nsis_script_path = self.nsis_install_dir.join("installer.nsi");
        if nsis_script_path.exists() {
            fs::remove_file(nsis_script_path)?;
        }

        Ok(())
    }
}

// Use the macro to automatically implement all common traits
crate::impl_common_builders!(NsisBuilder<'_>, common);

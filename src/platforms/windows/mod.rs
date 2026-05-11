//! Windows application bundler for Flutter projects.
//!
//! This module implements the `BundleBuilder` trait to package a Flutter application
//! for Windows into distributable formats:
//! - NSIS-based installer (`.exe`)
//! - Portable ZIP archive
//!
//! It handles:
//! - Building the Flutter app in release mode
//! - Locating or downloading NSIS toolchain
//! - Generating installer scripts from templates
//! - Creating checksum files for artifacts
//!
//! The builder assumes a standard Flutter Windows project layout
//! (e.g., `windows/runner/resources/app_icon.ico`).
//!
//! Note: MSI support is planned but not yet implemented.

mod nsis;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};

use crate::{
    cli::args::CommandOutputMode,
    config::{AppConfig, InstallerFormat, PacorConfig, WindowsConfig},
    core::{
        arch::Arch,
        builder::BundleBuilder,
        common::{
            ArtifactManager, BuildCleaner, BuildEnvironmentValidator, CommonBuilderImpl,
            CommonBuilderOperations, ConfigurationValidator, FlutterBuildOperations,
        },
        project::{FlutterProject, find_flutter_project_root},
    },
    error::{BuildError, ConfigurationError, PlatformError},
    platforms::{BuildArtifact, Platform, windows::nsis::NsisBuilder},
    utils::{
        archive::create_zip_archive,
        command::{execute_command, find_command_path, macros::command_args},
        fs::ensure_directory_exists,
    },
};

pub(crate) struct WindowsBuilder<'a> {
    /// Common builder functionality
    common: CommonBuilderImpl,
    app_config: &'a AppConfig,
    windows_config: &'a WindowsConfig,

    flutter_project: FlutterProject,

    project_root: PathBuf,
    flutter_build_output_dir: PathBuf,
    output_directory: PathBuf,

    tools_dir: PathBuf,

    target_arch: Arch,
}

impl<'a> BundleBuilder for WindowsBuilder<'a> {
    type Config = &'a PacorConfig;

    type Output = Vec<BuildArtifact>;

    /// Creates a new WindowsBuilder instance
    fn new(config: Self::Config) -> Result<Self> {
        let windows_config = match &config.platforms.windows {
            Some(windows_config) => windows_config,
            None => {
                return Err(ConfigurationError::PlatformNotEnabled {
                    platform: "Windows".to_string(),
                }
                .into());
            }
        };

        if !windows_config.is_enabled() {
            return Err(ConfigurationError::PlatformNotEnabled {
                platform: "Windows".to_string(),
            }
            .into());
        }

        let arch = Arch::current()?;

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_output_dir =
            flutter_project.get_flutter_build_path(&project_root, Platform::Windows, &arch);

        let output_directory = config.output_dir()?;

        let tools_dir = dirs::config_dir()
            .ok_or(PlatformError::BuildFailed {
                platform: "Windows".to_string(),
                reason: "Failed to get user config directory".to_string(),
            })?
            .join("pacor")
            .join("windows");

        info!(
            project = %flutter_project.name(),
            version = %flutter_project.version(),
            arch = %arch,
            output_dir = %output_directory.display(),
            tools_dir = %tools_dir.display(),
            "Initializing Windows builder"
        );

        Ok(Self {
            common: CommonBuilderImpl,
            app_config: &config.app,
            windows_config,

            project_root,
            flutter_project,

            flutter_build_output_dir,
            output_directory,
            tools_dir,

            target_arch: arch,
        })
    }

    /// Validates the builder configuration and prerequisites
    fn validate(&self) -> Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let windows_path = self.project_root.join("windows");
        let required_files = [
            pubspec_path.as_path(),
            windows_path.as_path(),
            &self.application_icon_path(),
        ];

        self.validate_build_environment(
            "Windows",
            &required_tools,
            &required_files,
            &self.output_directory,
        )?;

        // Validate signing config
        if self.should_sign_binaries() {
            self.validate_config_file_path(
                "platforms.windows.certificate_file",
                self.windows_config.certificate_path(),
            )?;

            let cert_password = self.windows_config.certificate_password();
            self.validate_required_config(
                "platforms.windows.certificate_password",
                &cert_password,
            )?;
        }

        Ok(())
    }

    /// Builds the Windows installer
    fn build(
        &self,
        skip_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!("Starting Windows bundle build...");

        // TODO: Remove this when MSI support is implemented
        if matches!(self.windows_config.installer_format(), InstallerFormat::Msi) {
            return Err(anyhow!("MSI installer support is not implemented yet"));
        }

        // Ensure output directory exists and validate permissions
        self.validate_directory_permissions(&self.output_directory, "Windows build output")?;
        ensure_directory_exists(&self.output_directory)?;
        trace!(output_dir = %self.output_directory.display(), "Output directory ensured");

        if !skip_flutter_build {
            self.build_flutter_application(&command_output_mode)?;
        } else {
            info!("Skipping Flutter build as requested by user");
        }

        let installer_artifact = self.build_installer(&command_output_mode)?;
        let mut artifacts = vec![installer_artifact];
        if self.windows_config.is_portable_build_enabled() {
            let portable_artifact = self.build_portable_archive(&command_output_mode)?;
            artifacts.push(portable_artifact);
        }

        info!("Windows bundle build completed successfully");
        Ok(artifacts)
    }

    /// Cleans the build directory
    fn clean(&self) -> Result<()> {
        self.clean_directory_contents(self.flutter_build_path())?;
        self.clean_directory_contents(&self.project_root().join("tmp"))?;

        Ok(())
    }

    /// Returns reference to the project root directory
    fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns reference to the Flutter build output directory
    fn flutter_build_path(&self) -> &Path {
        &self.flutter_build_output_dir
    }
}

impl WindowsBuilder<'_> {
    /// Returns the expected path to the application icon (`app_icon.ico`).
    fn application_icon_path(&self) -> PathBuf {
        self.project_root
            .join("windows")
            .join("runner")
            .join("resources")
            .join("app_icon.ico")
    }

    /// Finds the main `.exe` file in the Flutter build output.
    fn find_main_executable_name(&self) -> Result<String> {
        trace!(build_dir = %self.flutter_build_path().display(), "Scanning for main executable");

        let dir = fs::read_dir(self.flutter_build_path())
            .context("Failed to read build output directory")?;

        let target_filename = format!("{}.exe", self.flutter_project.name());

        for entry in dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && path.extension().is_some_and(|ext| ext == "exe")
                && let Some(filename) = path.file_name().and_then(|n| n.to_str())
                && filename == target_filename
            {
                debug!(executable = %filename, path = %path.display(), "Detected main executable");
                return Ok(filename.to_string());
            }
        }

        Err(BuildError::ExecutableNotFound.into())
    }

    /// Gets the default copyright string from the Runner.rc file.
    ///
    /// Returns the first `LegalCopyright` line found in the Runner.rc file, such as:
    ///
    /// ```
    ///     VALUE "LegalCopyright", "Copyright (C) 2025 com.YourCompany. All rights reserved." "\0"
    /// ```
    fn get_default_legal_copyright(&self) -> Result<String> {
        let runner_rc = self
            .project_root
            .join("windows")
            .join("runner")
            .join("Runner.rc");

        if !runner_rc.exists() {
            return Err(PlatformError::FileNotFound { path: runner_rc }.into());
        }

        let contents = fs::read_to_string(runner_rc)?;
        if let Some(copyright) = contents
            .lines()
            .find(|line| line.contains("VALUE \"LegalCopyright\", "))
        {
            return Ok(copyright
                .trim()
                .replace("VALUE \"LegalCopyright\", \"", "")
                .replace(".\" \"\\0\"", "")
                .trim()
                .to_string());
        }

        Err(PlatformError::InvalidConfiguration {
            platform: "Windows".to_string(),
            reason: "Could not find copyright information in Runner.rc".to_string(),
        }
        .into())
    }

    /// Builds the Windows application in release mode
    fn build_flutter_application(&self, command_output_mode: &CommandOutputMode) -> Result<()> {
        info!("Building Flutter application for Windows in release mode");

        let symbols_dir = tempfile::TempDir::with_prefix("pacor-flutter-symbols-")?;

        let args = command_args![
            "windows",
            "--release",
            "--obfuscate",
            "--split-debug-info",
            symbols_dir.path(),
        ];

        self.execute_flutter_build(&args, &self.project_root, command_output_mode)?;

        info!(
            build_path = %self.flutter_build_path().display(),
            "Windows app built successfully"
        );
        Ok(())
    }

    /// Builds the NSIS installer
    fn build_nsis_installer(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<BuildArtifact> {
        let nsis_install_dir = self.tools_dir.join("nsis");

        let icon_path = self.application_icon_path();

        let executable_name = self.find_main_executable_name()?;

        let default_legal_copyright = self.get_default_legal_copyright()?;

        let builder = NsisBuilder::new(
            self.app_config,
            &self.project_root,
            &self.flutter_project,
            &nsis_install_dir,
            &executable_name,
            &icon_path,
            &default_legal_copyright,
        );
        let artifact = builder.build(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.target_arch,
            self.flutter_build_path(),
            &self.output_directory,
            command_output_mode,
        )?;

        // Sign the installer if signing is configured
        if self.should_sign_binaries() {
            self.sign_binary(&artifact.file, command_output_mode)?;
        }

        // TODO: move to clean()?
        builder.clean()?;

        Ok(artifact)
    }

    /// Builds a portable ZIP archive of the application.
    fn build_portable_archive(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<BuildArtifact> {
        info!("Creating portable ZIP archive...");

        // TODO: check if main exe already signed
        // Sign the main exe if signing is configured
        if self.should_sign_binaries() {
            let exe_name = self.find_main_executable_name()?;
            let exe_path = self.flutter_build_path().join(exe_name);
            self.sign_binary(&exe_path, command_output_mode)?;
        }

        let artifact_name = self.generate_artifact_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.target_arch,
            "portable",
            "zip",
        );

        let zip_path = self.output_directory.join(artifact_name);

        create_zip_archive(self.flutter_build_path(), &zip_path)?;
        debug!(source = %self.flutter_build_path().display(), archive = %zip_path.display(), "ZIP archive created");

        // Use common artifact creation functionality
        let artifact = self.create_artifact_with_checksum(zip_path)?;

        info!(path = %artifact.file().display(), "Portable archive created");
        Ok(artifact)
    }

    /// Dispatches installer build based on configured type.
    fn build_installer(&self, command_output_mode: &CommandOutputMode) -> Result<BuildArtifact> {
        match self.windows_config.installer_format() {
            InstallerFormat::Nsis => self.build_nsis_installer(command_output_mode),
            InstallerFormat::Msi => Err(anyhow!(
                "MSI installer support is not implemented yet".to_string(),
            )),
        }
    }

    fn should_sign_binaries(&self) -> bool {
        self.windows_config.signing_tool_path().is_some()
            || self.windows_config.certificate_path().is_some()
    }

    /// Signs a Windows binary (EXE, MSI, etc.) using signtool or a custom signing tool.
    fn sign_binary(
        &self,
        binary_path: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        let sign_tool = if let Some(tool) = self.windows_config.signing_tool_path() {
            PathBuf::from(tool)
        } else {
            // Default to signtool.exe (assumes it's in PATH or Windows SDK installed)
            match find_command_path("signtool")? {
                Some(path) => path,
                None => {
                    warn!(
                        "sign_tool not configured and signtool not found in PATH; skipping signing"
                    );
                    return Ok(()); // Not an error if signing is optional
                }
            }
        };

        // 经过检测，运行到这里时签名配置一定存在
        let cert_file = self.windows_config.certificate_path().as_ref().unwrap();
        let password = self.windows_config.certificate_password().unwrap();

        // Basic signtool command:
        // signtool sign /fd SHA256 /f "cert.pfx" /p "password" "app.exe"
        let args = command_args![
            "sign",
            "/fd",
            "SHA256",
            "/f",
            cert_file,
            "/p",
            password,
            binary_path,
        ];

        debug!(binary = %binary_path.display(), "Signing binary...");
        execute_command(
            sign_tool,
            &args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )?;

        info!(binary = %binary_path.display(), "Binary signed successfully");
        Ok(())
    }
}

// Use the macro to automatically implement all common traits
crate::impl_common_builders!(WindowsBuilder<'_>, common);

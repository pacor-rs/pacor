//! Windows application bundler for Flutter projects.
//!
//! This module implements the `BundleBuilder` trait to package a Flutter
//! application for Windows into distributable formats:
//! - NSIS-based installer (`.exe`), see [`nsis`]
//! - Portable ZIP archive, whose entries live under a single
//!   `<name>-<version>-<arch>` directory
//!
//! Every artifact gets a `.sha1` file next to it. When code signing is
//! configured ([`signing`]) the application executable is signed *before* the
//! installer is compiled, because NSIS embeds the file contents: signing
//! afterwards would leave the installed copy unsigned.
//!
//! Both the executable name and the default copyright are read from the Flutter
//! runner project (see [`runner`]) rather than assumed to match `pubspec.yaml`.
//!
//! The builder assumes a standard Flutter Windows project layout
//! (e.g., `windows/runner/resources/app_icon.ico`).
//!
//! `installer_format` selects between the per-user NSIS installer and the
//! per-machine MSI packages; see `docs/MSI.md` for the MSI constraints.

#[macro_use]
extern crate tracing;

mod flutter;
mod msi;
mod nsis;
mod runner;
mod signing;

#[cfg(test)]
pub(crate) mod test_support;

use std::path::{Path, PathBuf};

use anyhow::Result;

use pacor_core::{
    config::{FlutterBuildMode, InstallerFormat, PacorConfig, WindowsConfig},
    core::{
        arch::Arch,
        builder::BundleBuilder,
        project::{FlutterProject, find_flutter_project_root},
    },
    error::{ConfigurationError, PlatformError, ValidationError},
    platforms::{BuildArtifact, Platform},
    utils::{
        archive::create_zip_archive_under,
        command::CommandOutputMode,
        fs::{clean_dir, ensure_directory_exists},
    },
};

use crate::msi::MsiBuilder;
use crate::nsis::NsisBuilder;

pub struct WindowsBuilder<'a> {
    /// Common builder functionality
    config: &'a PacorConfig,
    windows_config: &'a WindowsConfig,

    flutter_project: FlutterProject,

    project_root: PathBuf,
    flutter_build_output_dir: PathBuf,
    output_directory: PathBuf,

    tools_dir: PathBuf,

    target_arch: Arch,
    build_mode: FlutterBuildMode,
}

impl<'a> BundleBuilder for WindowsBuilder<'a> {
    type Config = &'a PacorConfig;

    type Output = Vec<BuildArtifact>;

    /// Creates a new WindowsBuilder instance
    fn new(config: Self::Config) -> Result<Self> {
        static DEFAULT_WINDOWS_CONFIG: std::sync::OnceLock<WindowsConfig> =
            std::sync::OnceLock::new();
        let windows_config = config
            .platforms
            .windows
            .as_ref()
            .unwrap_or_else(|| DEFAULT_WINDOWS_CONFIG.get_or_init(WindowsConfig::default));

        if !windows_config.is_enabled() {
            return Err(ConfigurationError::PlatformNotEnabled {
                platform: "Windows".to_string(),
            }
            .into());
        }

        let arch = Arch::current()?;
        let build_mode = config.build.flutter_build_mode;

        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_output_dir = flutter_project.get_flutter_build_path_for_mode(
            &project_root,
            Platform::Windows,
            &arch,
            build_mode,
        );

        let output_directory = config.output_dir()?;

        let tools_dir = pacor_core::tools::platform_cache_dir(Platform::Windows).ok_or(
            PlatformError::BuildFailed {
                platform: "Windows".to_string(),
                reason: "Failed to get user config directory".to_string(),
            },
        )?;

        info!(
            project = %flutter_project.name(),
            version = %flutter_project.version(),
            arch = %arch,
            mode = %build_mode,
            output_dir = %output_directory.display(),
            tools_dir = %tools_dir.display(),
            "Initializing Windows builder"
        );

        Ok(Self {
            config,
            windows_config,

            project_root,
            flutter_project,

            flutter_build_output_dir,
            output_directory,
            tools_dir,

            target_arch: arch,
            build_mode,
        })
    }

    /// Validates the builder configuration and prerequisites
    fn validate(&self) -> Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let windows_path = self.project_root.join("windows");
        let required_files = [pubspec_path.as_path(), windows_path.as_path()];

        pacor_core::packaging::validate_build_environment(
            "Windows",
            &required_tools,
            &required_files,
            &self.output_directory,
        )?;

        // NSIS and MSI both use the runner icon for the installer and the
        // shortcuts, so it cannot be replaced by a placeholder.
        let icon_path = self.application_icon_path();
        if !icon_path.exists() {
            return Err(ValidationError::FileValidation {
                file: icon_path,
                reason: "the Windows runner icon is required as the installer icon; restore \
                         windows/runner/resources/app_icon.ico (for example with \
                         `flutter create --platforms=windows .`)"
                    .to_string(),
            }
            .into());
        }

        // The WiX toolset is a sizeable download; say so instead of surprising
        // the user in the middle of a build.
        if matches!(self.windows_config.installer_format(), InstallerFormat::Msi)
            && !MsiBuilder::is_cached(&self.tools_dir)
        {
            info!(
                path = %MsiBuilder::cache_dir(&self.tools_dir).display(),
                "The pinned WiX toolset will be downloaded by the next MSI build"
            );
        }

        // Validate signing config, including that a signing tool is reachable.
        if self.should_sign_binaries() {
            signing::Signer::resolve(self.windows_config)?;
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

        // Ensure output directory exists and validate permissions
        pacor_core::packaging::validate_directory_permissions(
            &self.output_directory,
            "Windows build output",
        )?;
        ensure_directory_exists(&self.output_directory)?;
        trace!(output_dir = %self.output_directory.display(), "Output directory ensured");

        if !skip_flutter_build {
            flutter::build(&self.project_root, self.build_mode, &command_output_mode)?;
        } else {
            info!("Skipping Flutter build as requested by user");
        }

        // Everything below packages this executable, so resolve it once and
        // fail early with a useful message when it is not there.
        let executable = self.main_executable_path()?;

        // Both installer formats embed the file contents, so the application has
        // to be signed before the installer is compiled; the portable archive is
        // packed from the same signed file.
        if let Some(signer) = self.signer()? {
            signer.sign(&executable, &command_output_mode)?;
        }

        // Every installer is signed after it is produced, because signing
        // rewrites the file and the checksum next to it.
        let mut artifacts = self.build_installers(&command_output_mode)?;
        if let Some(signer) = self.signer()? {
            for artifact in &mut artifacts {
                signer.sign(artifact.file(), &command_output_mode)?;
                *artifact =
                    pacor_core::packaging::create_artifact_with_checksum(artifact.file().clone())?;
            }
        }

        if self.windows_config.is_portable_build_enabled() {
            let portable_artifact = self.build_portable_archive()?;
            artifacts.push(portable_artifact);
        }

        info!("Windows bundle build completed successfully");
        Ok(artifacts)
    }

    /// Cleans the build directory
    fn clean(&self) -> Result<()> {
        pacor_core::packaging::clean_directory_contents(self.flutter_build_path())?;

        // `--split-debug-info=tmp/` writes the symbols next to the project.
        let symbols_dir = self.project_root.join(flutter::SYMBOLS_DIR);
        if symbols_dir.exists() {
            clean_dir(&symbols_dir)?;
            debug!(path = %symbols_dir.display(), "Removed split debug symbols");
        }

        Ok(())
    }

    fn clean_build_directories(&self) -> &'static [&'static str] {
        &["build/windows"]
    }

    fn artifact_suffixes(&self) -> &'static [&'static str] {
        &[".msi", ".exe", ".zip"]
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

    /// Finds the application executable in the Flutter build output.
    fn main_executable_path(&self) -> Result<PathBuf> {
        let cmake_binary_name = runner::binary_name_from_cmake(
            &self.project_root.join("windows").join("CMakeLists.txt"),
        );

        runner::find_main_executable(
            self.flutter_build_path(),
            cmake_binary_name.as_deref(),
            self.flutter_project.name(),
        )
    }

    /// Name of the application executable, relative to the build output.
    fn find_main_executable_name(&self) -> Result<String> {
        let path = self.main_executable_path()?;

        path.file_name()
            .and_then(|name| name.to_str())
            .map(ToString::to_string)
            .ok_or_else(|| {
                PlatformError::InvalidConfiguration {
                    platform: "Windows".to_string(),
                    reason: format!("{} has no usable file name", path.display()),
                }
                .into()
            })
    }

    /// The copyright `flutter create` wrote into `Runner.rc`, if any.
    ///
    /// Only a fallback for the localized `copyright` metadata, so an unreadable
    /// or unusual `Runner.rc` is not an error.
    fn get_default_legal_copyright(&self) -> Option<String> {
        let runner_rc = self
            .project_root
            .join("windows")
            .join("runner")
            .join("Runner.rc");

        match runner::read_text_file(&runner_rc) {
            Ok(contents) => runner::parse_legal_copyright(&contents),
            Err(error) => {
                debug!(
                    path = %runner_rc.display(),
                    error = %error,
                    "No default copyright available from Runner.rc"
                );
                None
            }
        }
    }

    /// Builds the NSIS installer
    fn build_nsis_installer(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<BuildArtifact> {
        // The tag is part of the path: the templates cached here are only valid
        // for the contract pacor pins, and `ensure_template_files` skips files
        // that already exist. Without the tag a newer pacor would silently keep
        // using the previous tag's templates.
        let nsis_install_dir = self
            .tools_dir
            .join("nsis")
            .join(pacor_core::tools::TEMPLATE_REF);

        let executable_name = self.find_main_executable_name()?;
        let default_legal_copyright = self.get_default_legal_copyright().unwrap_or_default();
        let icon_path = self.application_icon_path();

        let builder = NsisBuilder::new(
            &self.config.app,
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

        Ok(artifact)
    }

    /// Builds the MSI installer(s), one per configured locale.
    ///
    /// The package is per-machine and needs administrator rights to install,
    /// unlike the per-user NSIS one; see `docs/MSI.md`.
    fn build_msi_installer(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<Vec<BuildArtifact>> {
        let wix_dir = MsiBuilder::cache_dir(&self.tools_dir);
        let executable_name = self.find_main_executable_name()?;
        let icon_path = self.application_icon_path();

        let builder = MsiBuilder::new(
            &self.config.app,
            self.windows_config,
            &self.flutter_project,
            &self.target_arch,
            &wix_dir,
            &icon_path,
            &executable_name,
        );

        builder.build(
            self.flutter_build_path(),
            &self.output_directory,
            command_output_mode,
        )
    }

    /// Builds a portable ZIP archive of the application.
    ///
    /// The archive holds a single top-level directory, so extracting it cannot
    /// scatter the application into an unrelated folder.
    fn build_portable_archive(&self) -> Result<BuildArtifact> {
        info!("Creating portable ZIP archive...");

        let artifact_name = pacor_core::packaging::artifact_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.target_arch,
            "portable",
            "zip",
        );

        let zip_path = self.output_directory.join(artifact_name);
        let archive_root = pacor_core::packaging::artifact_base_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.target_arch,
        );

        create_zip_archive_under(self.flutter_build_path(), Some(&archive_root), &zip_path)?;
        debug!(
            source = %self.flutter_build_path().display(),
            archive = %zip_path.display(),
            root = %archive_root,
            "ZIP archive created"
        );

        // Use common artifact creation functionality
        let artifact = pacor_core::packaging::create_artifact_with_checksum(zip_path)?;

        info!(path = %artifact.file().display(), "Portable archive created");
        Ok(artifact)
    }

    /// Dispatches installer build based on configured type.
    fn build_installers(
        &self,
        command_output_mode: &CommandOutputMode,
    ) -> Result<Vec<BuildArtifact>> {
        match self.windows_config.installer_format() {
            InstallerFormat::Nsis => Ok(vec![self.build_nsis_installer(command_output_mode)?]),
            InstallerFormat::Msi => self.build_msi_installer(command_output_mode),
        }
    }

    fn should_sign_binaries(&self) -> bool {
        self.windows_config.signing_tool_path().is_some()
            || self.windows_config.certificate_path().is_some()
    }

    /// Resolved signer, or `None` when code signing is not configured.
    fn signer(&self) -> Result<Option<signing::Signer<'_>>> {
        if !self.should_sign_binaries() {
            debug!("Code signing disabled: no certificate or tool configured");
            return Ok(None);
        }

        Ok(Some(signing::Signer::resolve(self.windows_config)?))
    }
}

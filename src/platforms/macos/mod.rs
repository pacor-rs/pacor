//! # macOS Application Builder Module
//!
//! This module provides comprehensive support for packaging Flutter macOS applications
//! into distributable DMG installers and updater archives. It handles the complete
//! macOS packaging workflow including code signing and Apple notarization.
//!
//! ## Features
//!
//! - **DMG Creation**: Packages Flutter .app bundles into professional DMG installers
//!   with drag-and-drop installation UI, custom backgrounds, and window sizing
//! - **Code Signing**: Applies digital signatures using configured developer identities
//!   for secure distribution outside the Mac App Store
//! - **Apple Notarization**: Submits DMG files to Apple's notarization service and
//!   staples tickets to avoid Gatekeeper warnings on macOS Catalina+
//! - **Updater Archives**: Creates compressed .tar.gz archives for auto-update systems
//! - **SHA1 Checksums**: Generates verification checksums for all build artifacts
//! - **Xcode Integration**: Updates AppInfo.xcconfig with custom application names
//!
//! ## Dependencies
//!
//! This module requires several macOS-specific tools to be available:
//! - Xcode Command Line Tools (`codesign`, `xcrun`, `notarytool`, `stapler`)
//! - The bundled `create-dmg` shell script for DMG creation
//! - Proper keychain setup for code signing and notarization credentials
//!
//! ## Usage
//!
//! The `MacOSAppBuilder` implements the `BundleBuilder` trait and is typically
//! used through the main build system. It requires a properly configured
//! `BundleConfig` with macOS-specific settings enabled.
//!
//! ## Security Considerations
//!
//! Code signing and notarization credentials are managed through macOS Keychain.
//! The builder never handles raw credentials directly, instead referencing
//! keychain profiles and signing identities by name.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::core::{
    arch::Arch,
    builder::BundleBuilder,
    common::{
        ArtifactManager, BuildEnvironmentValidator, CommonBuilderImpl, CommonBuilderOperations,
    },
    project::{FlutterProject, find_flutter_project_root},
};
use crate::error::{BundleError, Result};
use crate::platforms::{BuildArtifact, Platform};
use crate::utils::{
    archive::create_gzip_archive,
    command::{
        execute_command, execute_flutter_build_command,
        macros::{command_args, command_args_extend, command_args_push},
    },
    crypto::generate_sha1_checksum_file,
    fs::{copy_directory_with_parent, ensure_directory_exists, write_executable_file},
};
use crate::{cli::args::CommandOutputMode, config::AppConfig};
use crate::{
    config::{BundleConfig, MacOSBuildConfig},
    platforms::macos::plist::Plist,
};

mod localization;
mod plist;

// Embedded template resources for macOS packaging
const EULA_RESOURCES_TEMPLATE_XML: &str =
    include_str!("../../../tools/macos/support/eula-resources-template.xml");
const DMG_APPLESCRIPT_TEMPLATE: &str =
    include_str!("../../../tools/macos/support/template.applescript");

const CREATE_DMG_SHELL_SCRIPT_CONTENT: &str = include_str!("../../../tools/macos/create-dmg.sh");

/// Builder responsible for packaging a Flutter macOS application into a DMG installer
/// and generating an updater archive with optional code signing and notarization.
///
/// This builder handles the complete macOS packaging workflow:
/// 1. Prepares necessary tools and templates
/// 2. Builds the Flutter macOS application
/// 3. Creates a DMG installer
/// 4. Optionally code signs the DMG
/// 5. Optionally notarizes the DMG with Apple
/// 6. Generates updater archives and SHA1 checksums
pub(crate) struct MacOSAppBuilder {
    /// Common builder functionality
    common: CommonBuilderImpl,
    app_config: AppConfig,
    macos_config: MacOSBuildConfig,

    flutter_project: FlutterProject,

    project_root: PathBuf,
    flutter_build_dir: PathBuf,
    output_dir: PathBuf,

    tools_dir: PathBuf,
    create_dmg_script_path: PathBuf,

    arch: Arch,
}

impl BundleBuilder for MacOSAppBuilder {
    type Config = BundleConfig;
    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> Result<Self> {
        let macos_config = config.platforms.macos.clone();

        if !macos_config.enabled {
            return Err(BundleError::Platform(
                "macOS platform is not enabled in configuration".to_string(),
            ));
        }

        // Initialize project paths and metadata
        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_dir =
            flutter_project.get_flutter_build_path(&project_root, Platform::MacOS);

        let arch = Arch::current()?;
        let output_dir = config.output_dir()?;

        // Setup packaging tools directory in user's config
        let tools_dir = dirs::config_dir()
            .ok_or_else(|| {
                BundleError::Platform(
                    "Failed to determine user configuration directory".to_string(),
                )
            })?
            .join("pacor")
            .join("macos");
        let create_dmg_script_path = tools_dir.join("create-dmg.sh");

        info!(
            project_name = flutter_project.name(),
            project_version = flutter_project.version(),
            target_arch = %arch,
            output_directory = %output_dir.display(),
            "Initializing macOS application builder"
        );

        Ok(Self {
            common: CommonBuilderImpl::default(),
            app_config: config.app,
            macos_config,

            project_root,
            flutter_project,

            flutter_build_dir,
            output_dir,
            tools_dir,
            create_dmg_script_path,

            arch,
        })
    }

    fn build(
        &self,
        skip_flutter_build: bool,
        command_output_mode: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!("Starting macOS DMG packaging workflow");

        // Ensure output directory exists and validate permissions
        self.validate_directory_permissions(&self.output_dir, "macOS build output")?;
        ensure_directory_exists(&self.output_dir)?;
        trace!(
            output_directory = %self.output_dir.display(),
            "Output directory prepared"
        );

        // Locate the built .app bundle
        let app_bundle_path = self
            .flutter_build_dir
            .join(format!("{}.app", self.flutter_project.name()));
        trace!(
            app_bundle_path = %app_bundle_path.display(),
            "Located source .app bundle"
        );

        if !skip_flutter_build {
            // Prepare packaging tools and templates
            self.prepare_packaging_tools()?;

            // Build Flutter application
            self.build_flutter_app(&command_output_mode)?;

            // Add localization resources to build output
            self.add_localization_resources(&app_bundle_path)?;
        } else {
            info!("Skipping Flutter build as requested");

            if !self.flutter_build_path().exists() {
                return Err(BundleError::Platform(
                    "Flutter build output directory does not exist".to_string(),
                ));
            }
        }

        let artifact_filename_base = self.generate_artifact_filename_base();

        // Create DMG installer
        let dmg_installer_path = self.create_dmg_installer(
            &app_bundle_path,
            &artifact_filename_base,
            &command_output_mode,
        )?;

        // Code sign DMG if configured
        if self.should_sign_code() {
            self.sign_dmg(&dmg_installer_path, &command_output_mode)?;
        } else {
            debug!("Code signing disabled: no signing identity configured");
        }

        // Notarize DMG if configured
        if self.should_notarize() {
            self.notarize_dmg(&dmg_installer_path, &command_output_mode)?;
        } else {
            debug!("Notarization disabled: no Apple credentials configured");
        }

        // Create updater archive
        let updater_archive_path =
            self.create_updater_archive(&app_bundle_path, &artifact_filename_base)?;

        // Generate SHA1 checksums for verification using common functionality
        debug!("Generating SHA1 checksum files for build artifacts");
        let dmg_artifact = self.create_artifact_with_checksum(dmg_installer_path)?;
        let updater_artifact = self.create_artifact_with_checksum(updater_archive_path)?;
        info!(
            dmg_checksum = %dmg_artifact.sha1().display(),
            updater_checksum = %updater_artifact.sha1().display(),
            "SHA1 checksum files generated successfully"
        );

        // Return build artifacts
        Ok(vec![dmg_artifact, updater_artifact])
    }

    fn clean(&self) -> Result<()> {
        // TODO: Implement cleanup of temporary files and build artifacts
        trace!("macOS builder cleanup: no cleanup operations implemented yet");
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter", "xcodebuild", "codesign"];
        let required_files = [
            self.project_root.join("pubspec.yaml").as_path(),
            self.project_root.join("macos").as_path(),
        ];

        self.validate_build_environment(
            "macOS",
            &required_tools,
            &required_files,
            &self.output_dir,
        )?;

        Ok(())
    }

    fn project_root(&self) -> &Path {
        &self.project_root
    }

    fn flutter_build_path(&self) -> &Path {
        &self.flutter_build_dir
    }
}

impl MacOSAppBuilder {
    /// Prepares macOS packaging tools and templates in the user's configuration directory.
    ///
    /// Sets up the create-dmg script, EULA template, and AppleScript template required
    /// for DMG creation.
    fn prepare_packaging_tools(&self) -> Result<()> {
        debug!(
            tools_directory = %self.tools_dir.display(),
            "Preparing macOS packaging tools and templates"
        );

        // Create main tools directory
        ensure_directory_exists(&self.tools_dir)?;
        trace!(
            directory = %self.tools_dir.display(),
            "Packaging tools directory created"
        );

        // Create support subdirectory for templates
        let support_dir = self.tools_dir.join("support");
        ensure_directory_exists(&support_dir)?;
        trace!(
            directory = %support_dir.display(),
            "Templates support directory created"
        );

        // Write EULA resources template if it doesn't exist
        let eula_template_path = support_dir.join("eula-resources-template.xml");
        if !eula_template_path.exists() {
            trace!(
                file_path = %eula_template_path.display(),
                "Installing EULA resources template"
            );
            fs::write(&eula_template_path, EULA_RESOURCES_TEMPLATE_XML)?;
        }

        // Write AppleScript template if it doesn't exist
        let applescript_template_path = support_dir.join("template.applescript");
        if !applescript_template_path.exists() {
            trace!(
                file_path = %applescript_template_path.display(),
                "Installing AppleScript template"
            );
            fs::write(&applescript_template_path, DMG_APPLESCRIPT_TEMPLATE)?;
        }

        // Write create-dmg shell script with executable permissions if it doesn't exist
        if !self.create_dmg_script_path.exists() {
            trace!(
                script_path = %self.create_dmg_script_path.display(),
                "Installing create-dmg shell script with executable permissions"
            );
            write_executable_file(
                &self.create_dmg_script_path,
                CREATE_DMG_SHELL_SCRIPT_CONTENT.as_bytes(),
            )?;
        }

        info!("macOS packaging tools and templates are ready");
        Ok(())
    }

    /// Builds the Flutter macOS application in release mode with code obfuscation.
    ///
    /// Creates the .app bundle that will be packaged into the DMG installer.
    /// Debug symbols are split into a separate directory to reduce binary size.
    fn build_flutter_app(&self, command_output_mode: &CommandOutputMode) -> Result<()> {
        info!("Building Flutter macOS application in release mode with obfuscation");

        let build_args = vec![
            "macos",
            "--release",
            "--obfuscate",
            "--split-debug-info=tmp/", // Store debug info separately for smaller binary
        ];

        execute_flutter_build_command(&build_args, &self.project_root, command_output_mode)?;

        info!("Flutter macOS application built successfully");
        Ok(())
    }

    /// Creates a compressed `.tar.gz` archive of the .app bundle for auto-updates.
    ///
    /// This archive can be downloaded and extracted by update mechanisms to perform
    /// in-place application updates.
    fn create_updater_archive(
        &self,
        app_bundle_path: &Path,
        filename_base: &str,
    ) -> Result<PathBuf> {
        let archive_path = self
            .output_dir
            .join(format!("{}.app.tar.gz", filename_base));
        trace!(
            source_app = %app_bundle_path.display(),
            archive_destination = %archive_path.display(),
            "Creating compressed updater archive"
        );
        create_gzip_archive(app_bundle_path, &archive_path)?;

        info!(
            archive_path = %archive_path.display(),
            "Updater archive created successfully"
        );

        Ok(archive_path)
    }

    /// Creates a DMG installer package with drag-and-drop installation UI.
    ///
    /// The DMG includes customizable features such as background images and window sizing.
    /// Uses the create-dmg script to generate a user-friendly installer experience.
    fn create_dmg_installer(
        &self,
        app_bundle_path: &Path,
        filename_base: &str,
        command_output_mode: &CommandOutputMode,
    ) -> Result<PathBuf> {
        let dmg_path = self.output_dir.join(format!("{}.dmg", filename_base));

        info!(
            dmg_path = %dmg_path.display(),
            "Creating DMG installer package"
        );

        // Remove existing DMG to avoid conflicts
        if dmg_path.exists() {
            debug!(
                existing_dmg = %dmg_path.display(),
                "Removing existing DMG file before creating new one"
            );
            fs::remove_file(&dmg_path)?;
        }

        // Create temporary staging directory for DMG creation
        let temp_dir = tempfile::TempDir::with_prefix("pacor_dmg_")?;
        let staging_dir = temp_dir.path().join(self.flutter_project.name());
        trace!(
            temp_directory = %staging_dir.display(),
            "Created temporary staging directory for DMG creation"
        );

        // Copy app bundle to staging directory
        copy_directory_with_parent(app_bundle_path, &staging_dir)?;
        trace!(
            source = %app_bundle_path.display(),
            destination = %staging_dir.display(),
            "Copied .app bundle to temporary staging directory"
        );

        // Build create-dmg command arguments
        let volume_name = format!("{} Installer", self.flutter_project.name());
        let mut dmg_args = command_args!(
            capacity = 14,
            &self.create_dmg_script_path,
            "--volname",
            volume_name,
            "--sandbox-safe",
            "--app-drop-link",
            "480",
            "120",
        );

        // Add custom background image if configured
        if let Some(bg_image) = &self.macos_config.dmg_background {
            debug!(
                background_image = %bg_image,
                "Applying custom background image to DMG"
            );
            command_args_extend!(dmg_args, "--background", bg_image);
        }

        // Add custom window size if configured
        if let Some((width, height)) = self.macos_config.dmg_window_size {
            debug!(width, height, "Applying custom window size to DMG");
            command_args_extend!(dmg_args, "--window-size", width, height);
        }

        // Add output path and source directory
        command_args_extend!(dmg_args, dmg_path, staging_dir);

        // Execute create-dmg script
        debug!("Executing create-dmg script to build DMG installer");
        execute_command(
            "bash",
            dmg_args,
            Some(&self.project_root),
            None::<&[(&str, &str)]>,
            command_output_mode,
        )?;

        info!(
            dmg_path = %dmg_path.display(),
            "DMG installer package created successfully"
        );
        Ok(dmg_path)
    }

    /// Checks if code signing is enabled via a configured signing identity.
    fn should_sign_code(&self) -> bool {
        self.macos_config.codesign_identity.is_some()
    }

    /// Signs the DMG with the configured signing identity.
    ///
    /// Code signing is required for distribution outside the Mac App Store and
    /// is a prerequisite for notarization.
    fn sign_dmg(
        &self,
        dmg_file_path: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        if let Some(identity) = &self.macos_config.codesign_identity {
            info!(
                signing_identity = %identity,
                dmg_path = %dmg_file_path.display(),
                "Applying code signature to DMG installer"
            );

            let mut sign_args = command_args!(
                capacity = 4,
                "-s", // Signing identity
                identity,
                &dmg_file_path // File to sign
            );

            // Add verbose output if requested
            if command_output_mode == &CommandOutputMode::Verbose {
                command_args_push!(sign_args, "--verbose");
            }

            execute_command(
                "codesign",
                sign_args,
                None,
                None::<&[(&str, &str)]>,
                command_output_mode,
            )?;
            info!("DMG code signature applied successfully");
        }
        Ok(())
    }

    /// Checks if notarization is enabled via configured Apple credentials.
    fn should_notarize(&self) -> bool {
        self.macos_config.notarize_credentials.is_some()
    }

    /// Submits the DMG to Apple for notarization and staples the ticket.
    ///
    /// Notarization is required for distribution of signed apps outside the Mac App Store
    /// to avoid Gatekeeper warnings on macOS Catalina (10.15) and later.
    fn notarize_dmg(&self, dmg_path: &Path, command_output_mode: &CommandOutputMode) -> Result<()> {
        if let Some(keychain_profile) = &self.macos_config.notarize_credentials {
            info!(
                keychain_profile = %keychain_profile,
                dmg_path = %dmg_path.display(),
                "Submitting DMG to Apple for notarization"
            );

            // Submit DMG for notarization and wait for completion
            let mut notarization_arguments = command_args!(
                capacity = 7,
                "notarytool",
                "submit",
                &dmg_path,
                "--keychain-profile",
                keychain_profile,
                "--wait", // Wait for notarization to complete
            );
            if command_output_mode == &CommandOutputMode::Verbose {
                command_args_push!(notarization_arguments, "--verbose");
            }
            execute_command(
                "xcrun",
                notarization_arguments,
                None,
                None::<&[(&str, &str)]>,
                command_output_mode,
            )?;

            // Staple the notarization ticket for offline verification
            debug!("Stapling notarization ticket to DMG for offline verification");
            let mut staple_args = command_args!(capacity = 4, "stapler", "staple", &dmg_path);
            if command_output_mode == &CommandOutputMode::Verbose {
                command_args_push!(staple_args, "--verbose");
            }
            execute_command(
                "xcrun",
                staple_args,
                None,
                None::<&[(&str, &str)]>,
                command_output_mode,
            )?;

            info!("DMG notarization and stapling completed successfully");
        }
        Ok(())
    }

    /// Generates a standardized base filename for build artifacts.
    ///
    /// Format: `{project_name}-{version}-{architecture}`
    /// This method is kept for compatibility but now uses the common trait.
    fn generate_artifact_filename_base(&self) -> String {
        self.generate_artifact_name(
            self.flutter_project.name(),
            self.flutter_project.version(),
            &self.arch.to_string(),
            "",
        )
        .trim_end_matches(".")
        .to_string()
    }

    /// Adds localization resources to the built .app bundle.
    ///
    /// Creates `.lproj` directories for each locale and updates the Info.plist
    /// with localization metadata. Skips processing for single-language applications.
    fn add_localization_resources(&self, app_bundle_path: &Path) -> Result<()> {
        let contents_dir = app_bundle_path.join("Contents");

        let mut i18n = Vec::with_capacity(self.app_config.i18n().len());

        // Create localization directories and InfoPlist.strings files
        let resources_dir = contents_dir.join("Resources");
        for (locale, metadata) in self.app_config.i18n() {
            let lproj_dir = resources_dir.join(format!("{}.lproj", locale));
            ensure_directory_exists(&lproj_dir)?;
            let strings_file = lproj_dir.join("InfoPlist.strings");
            fs::write(&strings_file, metadata.to_string())?;
            i18n.push(locale.as_str());
        }

        // Update Info.plist with localization metadata
        let plist_path = contents_dir.join("Info.plist");
        let plist_content = fs::read_to_string(&plist_path)?;
        let mut plist = Plist::parse(&plist_content);
        plist.insert("CFBundleLocalizations", i18n);
        plist.insert(
            "CFBundleDevelopmentRegion",
            self.app_config.default_locale(),
        );

        fs::write(plist_path, plist.to_string())?;

        Ok(())
    }
}

// Use the macro to automatically implement all common traits
crate::impl_common_builders!(MacOSAppBuilder, common);

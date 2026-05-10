use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::Result;

use crate::{
    cli::args::CommandOutputMode,
    core::arch::Arch,
    utils::command::macros::{command_args, command_args_push},
};
use crate::{
    config::{AndroidConfig, PacorConfig},
    error::ConfigurationError,
};
use crate::{
    core::{
        builder::BundleBuilder,
        common::{
            ArtifactManager, BuildEnvironmentValidator, CommonBuilderImpl, CommonBuilderOperations,
        },
        project::{FlutterProject, find_flutter_project_root},
    },
    platforms::BuildArtifact,
    utils::{
        command::execute_flutter_build_command,
        fs::{clean_dir, ensure_directory_exists},
    },
};

mod abi;

pub(crate) use abi::TargetAbi;

/// Builder for Android platform bundles (APKs).
pub(crate) struct AndroidBuilder {
    /// Common builder functionality
    common: CommonBuilderImpl,
    android_config: AndroidConfig,
    project_root: PathBuf,
    flutter_build_path: PathBuf,
    output_directory: PathBuf,
    flutter_project: FlutterProject,
}

impl BundleBuilder for AndroidBuilder {
    type Config = PacorConfig;
    type Output = Vec<BuildArtifact>;

    /// Creates a new `AndroidBuilder` instance from the provided bundle configuration.
    fn new(config: Self::Config) -> Result<Self> {
        let android_config = match &config.platforms.android {
            Some(android_config) => android_config.clone(),
            None => {
                return Err(ConfigurationError::MissingRequired {
                    field: "platforms.android".to_string(),
                }
                .into());
            }
        };

        if !android_config.enabled {
            return Err(ConfigurationError::PlatformNotEnabled {
                platform: "Android".to_string(),
            }
            .into());
        }

        let arch = Arch::current()?;
        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_path =
            flutter_project.get_flutter_build_path(&project_root, super::Platform::Android, &arch);
        let output_directory = config.output_dir()?;

        Ok(Self {
            common: CommonBuilderImpl,
            android_config,
            project_root,
            flutter_project,
            flutter_build_path,
            output_directory,
        })
    }

    /// Builds the Android APK bundle.
    ///
    /// If `skip_flutter_build` is true, the Flutter build step will be skipped.
    /// If `verbose` is true, detailed build output will be shown.
    fn build(
        &self,
        skip_flutter_build: bool,
        command_output: CommandOutputMode,
    ) -> Result<Self::Output> {
        info!("Starting Android APK build...");

        // Ensure output directory exists and validate permissions
        self.validate_directory_permissions(&self.output_directory, "Android build output")?;
        ensure_directory_exists(&self.output_directory)?;

        // Build APK if not skipped
        if !skip_flutter_build {
            self.build_apk(&command_output)?;
        } else {
            info!("Skipping Flutter app build as requested");
        }

        // Copy APK to output directory
        let outputs = self.copy_apk_to_output()?;

        info!("Android APK build completed successfully");
        Ok(outputs)
    }

    /// Cleans the output directory by removing it entirely.
    fn clean(&self) -> Result<()> {
        clean_dir(&self.output_directory)?;

        Ok(())
    }

    /// Validates the builder configuration and build environment.
    fn validate(&self) -> Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter", "adb"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let android_path = self.project_root.join("android");
        let required_files = [pubspec_path.as_path(), android_path.as_path()];

        self.validate_build_environment(
            "Android",
            &required_tools,
            &required_files,
            &self.output_directory,
        )?;

        Ok(())
    }

    /// Returns the root path of the Flutter project.
    fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns the Flutter build output path for Android.
    fn flutter_build_path(&self) -> &Path {
        &self.flutter_build_path
    }
}

impl AndroidBuilder {
    /// Builds the Android APK using Flutter CLI.
    fn build_apk(&self, command_output: &CommandOutputMode) -> Result<()> {
        let symbols_dir = tempfile::TempDir::with_prefix("pacor-flutter-symbols-")?;

        let mut flutter_args = command_args![
            "apk",
            "--release",
            "--split-per-abi",
            "--split-debug-info",
            symbols_dir.path(),
        ];

        let target_platforms: Option<String> = if !self.android_config.target_abi.is_empty() {
            command_args_push!(flutter_args, "--target-platform");

            let mut platforms = Vec::new();
            for abi in &self.android_config.target_abi {
                platforms.push(abi.as_target_platform());
            }

            Some(platforms.join(","))
        } else {
            None
        };

        if let Some(ref platforms_str) = target_platforms {
            command_args_push!(flutter_args, platforms_str);
        }

        execute_flutter_build_command(&flutter_args, &self.project_root, command_output)?;
        Ok(())
    }

    /// Copies the built APK files to the output directory with appropriate naming.
    ///
    /// Returns a vector of tuples containing [apk_path, sha1_path] for each ABI.
    fn copy_apk_to_output(&self) -> Result<Vec<BuildArtifact>> {
        let target_abis = if self.android_config.target_abi.is_empty() {
            HashSet::from([TargetAbi::Arm64])
        } else {
            self.android_config.target_abi.clone()
        };

        let mut output_files = Vec::with_capacity(target_abis.len());

        for target_abi in target_abis {
            let apk_filename = format!("app-{}-release.apk", target_abi);
            let source_apk_path = self.flutter_build_path.join(&apk_filename);
            let target_apk_path = self.output_directory.join(format!(
                "{}-{}-{}.apk",
                self.flutter_project.name(),
                self.flutter_project.version(),
                target_abi
            ));

            std::fs::copy(&source_apk_path, &target_apk_path)?;

            // Sign APK if keystore configuration is provided
            if self.should_sign_apk() {
                self.sign_apk(&target_apk_path)?;
            }

            // Copy the corresponding SHA1 checksum file
            let source_sha1_path = self
                .flutter_build_path
                .join(format!("{}.sha1", apk_filename));
            let target_sha1_path = self.output_directory.join(format!(
                "{}-{}-{}.apk.sha1",
                self.flutter_project.name(),
                self.flutter_project.version(),
                target_abi,
            ));

            std::fs::copy(&source_sha1_path, &target_sha1_path)?;

            // Use common artifact creation functionality
            let artifact = self.create_artifact_with_checksum(target_apk_path)?;
            output_files.push(artifact);
        }

        Ok(output_files)
    }

    /// Determines whether APK signing should be performed based on keystore configuration.
    fn should_sign_apk(&self) -> bool {
        self.android_config.keystore_file.is_some()
    }

    /// Signs the APK using the configured keystore.
    ///
    /// Note: This is currently a placeholder implementation.
    fn sign_apk(&self, _apk_path: &Path) -> Result<()> {
        // TODO: Implement APK signing using jarsigner or apksigner
        warn!("APK signing functionality is not yet implemented");
        Ok(())
    }
}

// Use the macro to automatically implement all common traits
crate::impl_common_builders!(AndroidBuilder, common);

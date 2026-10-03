#[macro_use]
extern crate tracing;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::Result;

/// The only output format this backend produces.
pub const SUPPORTED_FORMAT: &str = "apk";

use pacor_core::{
    config::{AndroidConfig, PacorConfig},
    core::{
        arch::Arch,
        builder::BundleBuilder,
        project::{FlutterProject, find_flutter_project_root},
    },
    error::ConfigurationError,
    platforms::BuildArtifact,
    utils::{
        command::{
            CommandOutputMode, execute_flutter_build_command,
            macros::{command_args, command_args_push},
        },
        fs::ensure_directory_exists,
    },
};

pub use pacor_core::core::abi::TargetAbi;

/// Builder for Android platform bundles (APKs).
pub struct AndroidBuilder<'a> {
    android_config: AndroidConfig,
    project_root: PathBuf,
    flutter_build_path: PathBuf,
    output_directory: PathBuf,
    flutter_project: FlutterProject,
    _config: std::marker::PhantomData<&'a PacorConfig>,
}

impl<'a> BundleBuilder for AndroidBuilder<'a> {
    type Config = &'a PacorConfig;
    type Output = Vec<BuildArtifact>;

    fn new(config: Self::Config) -> Result<Self> {
        let android_config = config.platforms.android.clone().unwrap_or_default();

        if !android_config.enabled {
            return Err(ConfigurationError::PlatformNotEnabled {
                platform: "Android".to_string(),
            }
            .into());
        }

        let arch = Arch::current()?;
        let project_root = find_flutter_project_root()?;
        let flutter_project = FlutterProject::load_from_pubspec(&project_root)?;
        let flutter_build_path = flutter_project.get_flutter_build_path(
            &project_root,
            pacor_core::platforms::Platform::Android,
            &arch,
        );
        let output_directory = config.output_dir()?;

        Ok(Self {
            android_config,
            project_root,
            flutter_project,
            flutter_build_path,
            output_directory,
            _config: std::marker::PhantomData,
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
        pacor_core::packaging::validate_directory_permissions(
            &self.output_directory,
            "Android build output",
        )?;
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

    /// Android relies on `flutter clean`; there are no temporary files of our own.
    fn clean(&self) -> Result<()> {
        Ok(())
    }

    /// Validates the builder configuration and build environment.
    fn validate(&self) -> Result<()> {
        // Use common validation functionality
        let required_tools = ["flutter", "adb"];
        let pubspec_path = self.project_root.join("pubspec.yaml");
        let android_path = self.project_root.join("android");
        let required_files = [pubspec_path.as_path(), android_path.as_path()];

        pacor_core::packaging::validate_build_environment(
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

impl AndroidBuilder<'_> {
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

            if self.should_sign_apk() {
                self.sign_apk(&target_apk_path)?;
            }

            let artifact = pacor_core::packaging::create_artifact_with_checksum(target_apk_path)?;
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
    fn sign_apk(&self, apk_path: &Path) -> Result<()> {
        let keystore_file = self
            .android_config
            .keystore_file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("keystore_file not configured"))?;
        let keystore_password = self
            .android_config
            .keystore_password()
            .ok_or_else(|| anyhow::anyhow!("keystore_password not configured"))?;
        let key_alias = self
            .android_config
            .key_alias
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("key_alias not configured"))?;
        let _key_password = self
            .android_config
            .key_password()
            .ok_or_else(|| anyhow::anyhow!("key_password not configured"))?;

        // TODO: Implement APK signing using jarsigner or apksigner
        warn!(
            "APK signing not yet implemented: would sign {} with keystore {}",
            apk_path.display(),
            keystore_file
        );
        let _ = (keystore_password, key_alias);
        Ok(())
    }
}

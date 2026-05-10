//! Common builder traits and utilities for reducing platform-specific code duplication.
//!
//! This module provides shared functionality and patterns that are common across
//! different platform builders, following the DRY (Don't Repeat Yourself) principle.
//!
//! # Common Patterns Extracted
//!
//! 1. **Artifact Management**: Common file operations, checksums, and artifact creation
//! 2. **Validation Patterns**: Shared validation logic for files, directories, and configuration
//! 3. **Build Lifecycle**: Common build steps like environment setup, cleanup, and output handling
//! 4. **Template Processing**: Shared template rendering and processing utilities
//! 5. **Command Execution**: Common patterns for executing external tools and commands

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::args::CommandOutputMode;
use crate::core::arch::Arch;
use crate::error::{ConfigurationError, PackageError, PlatformError, Result, ValidationError};
use crate::platforms::BuildArtifact;
use crate::utils::crypto::generate_sha1_checksum_file;

/// Trait for common artifact management operations across platforms.
///
/// This trait provides shared functionality for creating, validating, and managing
/// build artifacts that is common across different platform builders.
pub(crate) trait ArtifactManager {
    fn generate_artifact_base_name(
        &self,
        project_name: &str,
        version: &str,
        target: &Arch,
    ) -> String {
        format!("{}-{}-{}", project_name, version, target)
    }

    /// Generate a standardized artifact filename based on project info and target.
    ///
    /// # Parameters
    /// - `project_name`: Name of the Flutter project
    /// - `version`: Project version string
    /// - `target`: Target platform/architecture identifier
    /// - `suffix`: Suffix to append to the filename
    /// - `extension`: File extension for the artifact
    ///
    /// # Returns
    /// A standardized filename in the format: `{project_name}-{version}-{target}-{suffix}.{extension}`
    #[cfg(windows)]
    fn generate_artifact_name(
        &self,
        project_name: &str,
        version: &str,
        target: &Arch,
        suffix: &str,
        extension: &str,
    ) -> String {
        format!(
            "{}-{}.{}",
            self.generate_artifact_base_name(project_name, version, target),
            suffix,
            extension
        )
    }

    /// Generate a standardized artifact filename based on project info and target.
    ///
    /// # Parameters
    /// - `project_name`: Name of the Flutter project
    /// - `version`: Project version string
    /// - `target`: Target platform/architecture identifier
    /// - `extension`: File extension for the artifact
    ///
    /// # Returns
    /// A standardized filename in the format: `{project_name}-{version}-{target}.{extension}`
    #[cfg(unix)]
    fn generate_artifact_name(
        &self,
        project_name: &str,
        version: &str,
        target: &Arch,
        extension: &str,
    ) -> String {
        format!(
            "{}.{}",
            self.generate_artifact_base_name(project_name, version, target),
            extension
        )
    }

    /// Create a build artifact with file and checksum.
    ///
    /// This method creates a `BuildArtifact` instance and generates the corresponding
    /// SHA1 checksum file for integrity verification.
    ///
    /// # Parameters
    /// - `artifact_path`: Path to the main artifact file
    ///
    /// # Returns
    /// A `BuildArtifact` containing both the artifact and checksum file paths
    ///
    /// # Errors
    /// Returns an error if checksum generation fails
    fn create_artifact_with_checksum(&self, artifact_path: PathBuf) -> Result<BuildArtifact> {
        let checksum_path = generate_sha1_checksum_file(&artifact_path).map_err(|e| {
            PackageError::Build(crate::error::BuildError::ArtifactGenerationFailed {
                artifact: artifact_path.display().to_string(),
                reason: format!("Failed to generate checksum: {}", e),
            })
        })?;

        Ok(BuildArtifact::new(artifact_path, checksum_path))
    }

    /// Validate that required artifact files exist and are accessible.
    ///
    /// # Parameters
    /// - `paths`: Collection of file paths that must exist
    ///
    /// # Errors
    /// Returns validation error if any required file is missing
    fn validate_required_files<P, I>(&self, paths: I) -> Result<()>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        for path in paths {
            let path_ref = path.as_ref();
            if !path_ref.exists() {
                return Err(PackageError::Validation(ValidationError::FileValidation {
                    file: path_ref.to_path_buf(),
                    reason: "Required file does not exist".to_string(),
                }));
            }
        }
        Ok(())
    }
}

/// Trait for common build environment validation across platforms.
///
/// This trait provides shared validation logic that can be reused by different
/// platform builders to ensure their build environment is properly configured.
pub(crate) trait BuildEnvironmentValidator {
    /// Validate that a tool is available in the system PATH.
    ///
    /// # Parameters
    /// - `tool_name`: Name of the tool executable to find
    /// - `platform`: Platform name for error context
    ///
    /// # Errors
    /// Returns platform error if the tool is not found
    fn validate_tool_availability(&self, tool_name: &str, platform: &str) -> Result<PathBuf> {
        match crate::utils::command::find_command_path(tool_name) {
            Ok(Some(path)) => Ok(path),
            Ok(None) => Err(PackageError::Platform(PlatformError::ToolNotFound {
                tool: tool_name.to_string(),
                platform: platform.to_string(),
            })),
            Err(_) => Err(PackageError::Platform(PlatformError::ToolNotFound {
                tool: tool_name.to_string(),
                platform: platform.to_string(),
            })),
        }
    }

    /// Validate directory permissions for build operations.
    ///
    /// # Parameters
    /// - `directory`: Directory path to validate
    /// - `operation`: Description of the operation requiring access
    ///
    /// # Errors
    /// Returns validation error if directory access is insufficient
    fn validate_directory_permissions(&self, directory: &Path, operation: &str) -> Result<()> {
        if !directory.exists() {
            return Err(PackageError::Validation(ValidationError::FileValidation {
                file: directory.to_path_buf(),
                reason: "Directory does not exist".to_string(),
            }));
        }

        // Test write access by attempting to create a temporary file
        let test_file = directory.join(".pacor_permission_test");
        match std::fs::write(&test_file, "test") {
            Ok(()) => {
                // Clean up test file
                let _ = std::fs::remove_file(&test_file);
                Ok(())
            }
            Err(_) => Err(PackageError::Validation(
                ValidationError::PermissionDenied {
                    path: directory.to_path_buf(),
                    operation: operation.to_string(),
                },
            )),
        }
    }
}

/// Trait for common cleanup operations across platforms.
///
/// This trait provides shared cleanup patterns that can be reused by different
/// platform builders to ensure consistent and thorough cleanup.
pub(crate) trait BuildCleaner {
    /// Clean a directory by removing all its contents but keeping the directory itself.
    ///
    /// # Parameters
    /// - `directory`: Directory to clean
    ///
    /// # Errors
    /// Returns error if cleanup fails due to permissions or I/O issues
    fn clean_directory_contents(&self, directory: &Path) -> Result<()> {
        if !directory.exists() {
            return Ok(());
        }

        if !directory.is_dir() {
            return Err(PackageError::Build(
                crate::error::BuildError::CleanupFailed {
                    reason: format!("{} is not a directory", directory.display()),
                },
            ));
        }

        let entries: std::fs::ReadDir = std::fs::read_dir(directory).map_err(|e| {
            PackageError::Build(crate::error::BuildError::CleanupFailed {
                reason: format!("Failed to read directory {}: {}", directory.display(), e),
            })
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                PackageError::Build(crate::error::BuildError::CleanupFailed {
                    reason: format!("Failed to read directory entry: {}", e),
                })
            })?;

            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).map_err(|e| {
                    PackageError::Build(crate::error::BuildError::CleanupFailed {
                        reason: format!("Failed to remove directory {}: {}", path.display(), e),
                    })
                })?;
            } else {
                std::fs::remove_file(&path).map_err(|e| {
                    PackageError::Build(crate::error::BuildError::CleanupFailed {
                        reason: format!("Failed to remove file {}: {}", path.display(), e),
                    })
                })?;
            }
        }

        Ok(())
    }
}

/// Trait for common configuration validation patterns.
///
/// This trait provides shared configuration validation logic that can be reused
/// across different platform builders.
pub(crate) trait ConfigurationValidator {
    /// Validate that required configuration fields are present and valid.
    ///
    /// # Parameters
    /// - `field_name`: Name of the configuration field
    /// - `value`: Optional value to validate
    ///
    /// # Errors
    /// Returns configuration error if the field is missing
    fn validate_required_config<T>(&self, field_name: &str, value: &Option<T>) -> Result<()> {
        if value.is_none() {
            return Err(PackageError::Configuration(
                ConfigurationError::MissingRequired {
                    field: field_name.to_string(),
                },
            ));
        }
        Ok(())
    }

    /// Validate that a file path in configuration exists and is accessible.
    ///
    /// # Parameters
    /// - `field_name`: Name of the configuration field
    /// - `path`: Optional path to validate
    ///
    /// # Errors
    /// Returns configuration error if the path is missing or invalid
    fn validate_config_file_path(&self, field_name: &str, path: &Option<PathBuf>) -> Result<()> {
        match path {
            None => Err(PackageError::Configuration(
                ConfigurationError::MissingRequired {
                    field: field_name.to_string(),
                },
            )),
            Some(p) if !p.exists() => Err(PackageError::Configuration(
                ConfigurationError::FileNotFound { path: p.clone() },
            )),
            Some(_) => Ok(()),
        }
    }

    /// Validate platform-specific configuration consistency.
    ///
    /// This is a placeholder for platform-specific validation logic.
    /// Platform builders can override this method to implement their own
    /// validation rules.
    fn validate_platform_config(&self) -> Result<()> {
        // Default implementation - platforms should override as needed
        Ok(())
    }
}

/// Trait for common Flutter build operations.
///
/// This trait provides shared Flutter build functionality that can be reused
/// across different platform builders.
pub(crate) trait FlutterBuildOperations {
    /// Execute a Flutter build command with common error handling.
    ///
    /// # Parameters
    /// - `args`: Flutter build arguments
    /// - `project_root`: Root directory of the Flutter project
    /// - `command_output`: Output mode for the command
    ///
    /// # Errors
    /// Returns build error if the Flutter build fails
    fn execute_flutter_build(
        &self,
        args: &[OsString],
        project_root: &Path,
        command_output: &CommandOutputMode,
    ) -> Result<()> {
        crate::utils::command::execute_flutter_build_command(args, project_root, command_output)
            .map_err(|e| {
                PackageError::Build(crate::error::BuildError::FlutterBuildFailed {
                    code: -1, // We don't have access to the actual exit code here
                    output: e.to_string(),
                })
            })
    }
}

/// Composite trait that combines all common builder functionality.
///
/// Platform-specific builders can implement this trait to get access to all
/// common functionality without having to implement each trait individually.
pub(crate) trait CommonBuilderOperations:
    ArtifactManager
    + BuildEnvironmentValidator
    + BuildCleaner
    + ConfigurationValidator
    + FlutterBuildOperations
{
    /// Perform comprehensive pre-build validation.
    ///
    /// This method combines multiple validation checks that are common
    /// across platforms.
    ///
    /// # Parameters
    /// - `platform_name`: Name of the platform for error context
    /// - `required_tools`: List of required tools to validate
    /// - `required_files`: List of required files to validate
    /// - `output_directory`: Output directory to validate permissions for
    ///
    /// # Errors
    /// Returns the first validation error encountered
    fn validate_build_environment(
        &self,
        platform_name: &str,
        required_tools: &[&str],
        required_files: &[&Path],
        output_directory: &Path,
    ) -> Result<()> {
        // Validate required tools
        for tool in required_tools {
            self.validate_tool_availability(tool, platform_name)?;
        }

        // Validate required files
        self.validate_required_files(required_files.iter())?;

        // Validate output directory permissions
        if !output_directory.exists() {
            warn!(
                "Output directory does not exist, creating: {}",
                output_directory.display()
            );
            std::fs::create_dir_all(output_directory)
                .inspect_err(|e| error!("Failed to create output directory: {e}"))?;
        }
        self.validate_directory_permissions(output_directory, "build output")?;

        // Platform-specific validation
        self.validate_platform_config()?;

        Ok(())
    }
}

/// Default implementation of CommonBuilderOperations for any type that implements
/// all the individual traits.
impl<T> CommonBuilderOperations for T where
    T: ArtifactManager
        + BuildEnvironmentValidator
        + BuildCleaner
        + ConfigurationValidator
        + FlutterBuildOperations
{
}

/// A default implementation struct that provides all common builder functionality.
///
/// Platform builders can use this struct as a composition member to get access
/// to all common functionality without having to implement the traits themselves.
#[derive(Debug)]
pub(crate) struct CommonBuilderImpl;

impl ArtifactManager for CommonBuilderImpl {}
impl BuildEnvironmentValidator for CommonBuilderImpl {}
impl BuildCleaner for CommonBuilderImpl {}
impl ConfigurationValidator for CommonBuilderImpl {}
impl FlutterBuildOperations for CommonBuilderImpl {}

/// Helper macro for implementing common builder traits on platform builders.
///
/// This macro generates boilerplate implementations that delegate to a
/// `CommonBuilderImpl` instance.
///
/// # Usage
///
/// ```rust
/// struct MyPlatformBuilder {
///     common: CommonBuilderImpl,
///     // ... platform-specific fields
/// }
///
/// impl_common_builders!(MyPlatformBuilder, common);
/// ```
#[macro_export]
macro_rules! impl_common_builders {
    ($builder:ty, $common_field:ident) => {
        impl $crate::core::common::ArtifactManager for $builder {
            fn generate_artifact_base_name(
                &self,
                project_name: &str,
                version: &str,
                target: &$crate::core::arch::Arch,
            ) -> String {
                self.$common_field
                    .generate_artifact_base_name(project_name, version, target)
            }

            #[cfg(windows)]
            fn generate_artifact_name(
                &self,
                project_name: &str,
                version: &str,
                target: &$crate::core::arch::Arch,
                suffix: &str,
                extension: &str,
            ) -> String {
                self.$common_field.generate_artifact_name(
                    project_name,
                    version,
                    target,
                    suffix,
                    extension,
                )
            }

            #[cfg(unix)]
            fn generate_artifact_name(
                &self,
                project_name: &str,
                version: &str,
                target: &$crate::core::arch::Arch,
                extension: &str,
            ) -> String {
                self.$common_field
                    .generate_artifact_name(project_name, version, target, extension)
            }

            fn create_artifact_with_checksum(
                &self,
                artifact_path: PathBuf,
            ) -> $crate::error::Result<$crate::platforms::BuildArtifact> {
                self.$common_field
                    .create_artifact_with_checksum(artifact_path)
            }

            fn validate_required_files<P, I>(&self, paths: I) -> $crate::error::Result<()>
            where
                P: AsRef<Path>,
                I: IntoIterator<Item = P>,
            {
                self.$common_field.validate_required_files(paths)
            }
        }

        impl $crate::core::common::BuildEnvironmentValidator for $builder {
            fn validate_tool_availability(
                &self,
                tool_name: &str,
                platform: &str,
            ) -> $crate::error::Result<PathBuf> {
                self.$common_field
                    .validate_tool_availability(tool_name, platform)
            }

            fn validate_directory_permissions(
                &self,
                directory: &Path,
                operation: &str,
            ) -> $crate::error::Result<()> {
                self.$common_field
                    .validate_directory_permissions(directory, operation)
            }
        }

        impl $crate::core::common::BuildCleaner for $builder {
            fn clean_directory_contents(&self, directory: &Path) -> $crate::error::Result<()> {
                self.$common_field.clean_directory_contents(directory)
            }
        }

        impl $crate::core::common::ConfigurationValidator for $builder {
            fn validate_required_config<T>(
                &self,
                field_name: &str,
                value: &Option<T>,
            ) -> $crate::error::Result<()> {
                self.$common_field
                    .validate_required_config(field_name, value)
            }

            fn validate_config_file_path(
                &self,
                field_name: &str,
                path: &Option<PathBuf>,
            ) -> $crate::error::Result<()> {
                self.$common_field
                    .validate_config_file_path(field_name, path)
            }

            fn validate_platform_config(&self) -> $crate::error::Result<()> {
                self.$common_field.validate_platform_config()
            }
        }

        impl $crate::core::common::FlutterBuildOperations for $builder {
            fn execute_flutter_build(
                &self,
                args: &[std::ffi::OsString],
                project_root: &Path,
                command_output: &$crate::cli::args::CommandOutputMode,
            ) -> $crate::error::Result<()> {
                self.$common_field
                    .execute_flutter_build(args, project_root, command_output)
            }
        }
    };
}

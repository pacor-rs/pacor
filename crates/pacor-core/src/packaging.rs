//! Shared helpers used by packaging backends.
//!
//! These used to live behind five traits, a composite trait, a zero-sized
//! `CommonBuilderImpl` and an `impl_common_builders!` delegation macro. None of
//! that carried state, so they are plain functions again.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::Result;

use crate::core::arch::Arch;
use crate::error::{BuildError, ConfigurationError, Error, PlatformError, ValidationError};
use crate::platforms::BuildArtifact;
use crate::utils::command::{CommandOutputMode, execute_flutter_build_command, find_command_path};
use crate::utils::crypto::generate_sha1_checksum_file;

/// Standard artifact name: [`artifact_base_name`] plus an optional `-{suffix}`
/// and the extension.
pub fn artifact_name(
    project_name: &str,
    version: &str,
    target: &Arch,
    suffix: &str,
    extension: &str,
) -> String {
    let base = artifact_base_name(project_name, version, target);
    if suffix.is_empty() {
        format!("{base}.{extension}")
    } else {
        format!("{base}-{suffix}.{extension}")
    }
}

/// Base (extension-less) artifact name: `{project}-{version}-{target}`.
pub fn artifact_base_name(project_name: &str, version: &str, target: &Arch) -> String {
    format!("{project_name}-{version}-{target}")
}

/// Generate the SHA1 checksum file next to `artifact_path`.
pub fn create_artifact_with_checksum(artifact_path: PathBuf) -> Result<BuildArtifact> {
    let checksum_path = generate_sha1_checksum_file(&artifact_path).map_err(|error| {
        Error::Build(BuildError::ArtifactGeneration {
            artifact: artifact_path.display().to_string(),
            reason: format!("failed to generate checksum: {error}"),
        })
    })?;

    Ok(BuildArtifact::new(artifact_path, checksum_path))
}

/// Verify that every path exists.
pub fn validate_required_files<P, I>(paths: I) -> Result<()>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = P>,
{
    for path in paths {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::Validation(ValidationError::FileValidation {
                file: path.to_path_buf(),
                reason: "required file does not exist".to_string(),
            }));
        }
    }
    Ok(())
}

/// Locate a tool on `PATH`, reporting a platform-specific error otherwise.
pub fn find_tool(tool: &str, platform: &str) -> Result<PathBuf> {
    match find_command_path(tool) {
        Ok(Some(path)) => Ok(path),
        _ => Err(Error::Platform(PlatformError::ToolNotFound {
            tool: tool.to_string(),
            platform: platform.to_string(),
        })),
    }
}

/// Check that a directory exists and is writable.
pub fn validate_directory_permissions(directory: &Path, operation: &str) -> Result<()> {
    if !directory.exists() {
        return Err(Error::Validation(ValidationError::FileValidation {
            file: directory.to_path_buf(),
            reason: "directory does not exist".to_string(),
        }));
    }

    let probe = directory.join(".pacor_permission_test");
    match std::fs::write(&probe, b"test") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(Error::Validation(ValidationError::PermissionDenied {
            path: directory.to_path_buf(),
            operation: operation.to_string(),
        })),
    }
}

/// Create `directory` if needed and check that it is writable.
pub fn prepare_output_directory(directory: &Path) -> Result<()> {
    if !directory.exists() {
        std::fs::create_dir_all(directory)?;
    }
    validate_directory_permissions(directory, "build output")
}

/// Remove the contents of a directory, keeping the directory itself.
pub fn clean_directory_contents(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    if !directory.is_dir() {
        return Err(Error::Build(BuildError::Cleanup {
            reason: format!("{} is not a directory", directory.display()),
        }));
    }

    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

/// Common pre-build validation: required tools, required files and a writable
/// output directory.
pub fn validate_build_environment(
    platform: &str,
    required_tools: &[&str],
    required_files: &[&Path],
    output_directory: &Path,
) -> Result<()> {
    for tool in required_tools {
        find_tool(tool, platform)?;
    }

    validate_required_files(required_files.iter().copied())?;
    prepare_output_directory(output_directory)
}

/// Run `flutter build` with the given arguments.
pub fn flutter_build(
    args: &[OsString],
    project_root: &Path,
    command_output: &CommandOutputMode,
    platform: &str,
) -> Result<()> {
    execute_flutter_build_command(args, project_root, command_output).map_err(|error| {
        Error::Build(BuildError::FlutterBuild {
            output: error.to_string(),
            platform: platform.to_string(),
        })
    })
}

/// Require that an optional configuration value is present.
pub fn require_config<T>(field: &str, value: &Option<T>) -> Result<()> {
    if value.is_none() {
        return Err(Error::Configuration(ConfigurationError::MissingRequired {
            field: field.to_string(),
        }));
    }
    Ok(())
}

/// Require that a configured file path exists.
pub fn require_existing_path(field: &str, path: &Option<PathBuf>) -> Result<()> {
    match path {
        None => Err(Error::Configuration(ConfigurationError::MissingRequired {
            field: field.to_string(),
        })),
        Some(path) if !path.exists() => {
            Err(Error::Configuration(ConfigurationError::ValidationFailed {
                field: field.to_string(),
                reason: format!("{} does not exist", path.display()),
            }))
        }
        Some(_) => Ok(()),
    }
}

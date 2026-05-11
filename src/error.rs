//! Error handling for the Pacor cross-platform packaging tool.
//!
//! This module provides a comprehensive error system with fine-grained error types
//! to improve debugging and error recovery capabilities across the application.

use std::{ffi::OsString, path::PathBuf};
use thiserror::Error;

/// Result type alias for Pacor operations.
pub type Result<T> = std::result::Result<T, PackageError>;

/// Main error enum for all Pacor operations.
///
/// This enum provides fine-grained error categorization to improve error handling,
/// debugging, and user experience. Each category contains specific error variants
/// that provide detailed context about what went wrong.
#[derive(Error, Debug)]
pub enum PackageError {
    // Flutter project-related errors
    #[error("Project error: {0}")]
    Project(#[from] ProjectError),

    // Configuration-related errors
    #[error("Configuration error: {0}")]
    Configuration(#[from] ConfigurationError),

    // Build process errors
    #[error("Build error: {0}")]
    Build(#[from] BuildError),

    // Platform-specific errors
    #[error("Platform error: {0}")]
    Platform(#[from] PlatformError),

    #[error("Unsupported architecture: {0}")]
    Architecture(String),

    // Validation errors
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    // System I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    // Network-related errors
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[error("Network error: {0}")]
    Network(#[from] ureq::Error),

    // Compression/archive errors
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[error("Archive error: {0}")]
    Archive(#[from] zip::result::ZipError),

    // File system path errors
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[error("Path error: {0}")]
    Path(#[from] std::path::StripPrefixError),

    // Directory traversal errors
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[error("Directory traversal error: {0}")]
    DirectoryTraversal(#[from] walkdir::Error),

    // Command execution errors
    #[error("Command error: {0}")]
    Command(#[from] CommandError),

    // Threading errors
    #[cfg(windows)]
    #[error("Threading error: {0}")]
    Threading(String),
}

#[derive(Error, Debug)]
pub enum ProjectError {
    #[error(
        "Could not find pubspec.yaml. Please make sure you are running this inside a Flutter project."
    )]
    RootNotFound,
    #[error("Flutter is not installed or not in PATH")]
    FlutterNotInstalled,
}

/// Configuration-related errors with specific context.
#[derive(Error, Debug)]
pub enum ConfigurationError {
    #[error("Platform not enabled: {platform}")]
    PlatformNotEnabled { platform: String },

    #[error("Configuration file not found at {}", path.display())]
    FileNotFound { path: PathBuf },

    #[error("Invalid configuration format in {}: {reason}",file.display())]
    InvalidFormat { file: PathBuf, reason: String },

    #[error("Configuration validation failed: {field} - {reason}")]
    ValidationFailed { field: String, reason: String },

    #[error("Missing required configuration: {field}")]
    MissingRequired { field: String },

    #[error("Configuration file already exists at {}. Use --force to overwrite", path.display())]
    FileAlreadyExists { path: PathBuf },

    #[error("Failed to serialize configuration: {reason}")]
    SerializationFailed { reason: String },

    #[error("Failed to deserialize configuration: {reason}")]
    DeserializationFailed { reason: String },

    #[error("Failed to write configuration to {}: {reason}",path.display())]
    WriteFailed { path: PathBuf, reason: String },
}

/// Build process errors with specific context.
#[derive(Error, Debug)]
pub enum BuildError {
    #[error("No .exe file found in build output")]
    ExecutableNotFound,

    #[error("Flutter build failed with exit code {code}: {output}")]
    FlutterBuildFailed { code: i32, output: String },

    #[error("Failed to generate build artifact {artifact}: {reason}")]
    ArtifactGenerationFailed { artifact: String, reason: String },

    #[error("Build environment setup failed: {reason}")]
    EnvironmentSetupFailed { reason: String },

    #[error("Template processing failed for {template}: {reason}")]
    TemplateProcessingFailed { template: String, reason: String },

    #[error("Build cleanup failed: {reason}")]
    CleanupFailed { reason: String },
}

/// Platform-specific errors with detailed context.
#[derive(Error, Debug)]
pub enum PlatformError {
    #[error("Unsupported platform: {platform}")]
    UnsupportedPlatform { platform: String },

    #[error("Platform tool not found: {tool} (required for {platform})")]
    ToolNotFound { tool: String, platform: String },

    #[error("File not found: {}", path.display())]
    FileNotFound { path: PathBuf },

    #[error("Invalid platform configuration for {platform}: {reason}")]
    InvalidConfiguration { platform: String, reason: String },

    #[error("Platform-specific build failed for {platform}: {reason}")]
    BuildFailed { platform: String, reason: String },

    #[cfg(target_os = "macos")]
    #[error("Invalid XCConfig format at line {line}: {content}")]
    InvalidXcconfigFormat { line: usize, content: String },
}

/// Validation errors for project and environment checks.
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Project validation failed: {reason}")]
    ProjectValidation { reason: String },

    #[error("Flutter project not found in {}", path.display())]
    FlutterProjectNotFound { path: PathBuf },

    #[error("Invalid project structure: {reason}")]
    InvalidProjectStructure { reason: String },

    #[error("File validation failed for {}: {reason}", file.display())]
    FileValidation { file: PathBuf, reason: String },

    #[error("Permission denied for {}: {operation}", path.display())]
    PermissionDenied { path: PathBuf, operation: String },
}

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Failed to execute {}: {error}",command.display())]
    Execute {
        command: OsString,
        error: std::io::Error,
    },

    #[error("Command {} failed: {exit_info}", command.display())]
    Failed {
        command: OsString,
        exit_info: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_error_display() {
        let err = ProjectError::RootNotFound;
        assert!(err.to_string().contains("pubspec.yaml"));
    }

    #[test]
    fn test_configuration_error_display() {
        let err = ConfigurationError::MissingRequired {
            field: "test_field".to_string(),
        };
        assert!(err.to_string().contains("test_field"));
    }

    #[test]
    fn test_build_error_display() {
        let err = BuildError::FlutterBuildFailed {
            code: 1,
            output: "error message".to_string(),
        };
        assert!(err.to_string().contains("1"));
    }

    #[test]
    fn test_platform_error_display() {
        let err = PlatformError::UnsupportedPlatform {
            platform: "freebsd".to_string(),
        };
        assert!(err.to_string().contains("freebsd"));
    }

    #[test]
    fn test_package_error_from_project() {
        let err: PackageError = ProjectError::FlutterNotInstalled.into();
        assert!(matches!(err, PackageError::Project(_)));
    }

    #[test]
    fn test_package_error_from_command() {
        let cmd_err = CommandError::Failed {
            command: OsString::from("test"),
            exit_info: "exit code 1".to_string(),
        };
        let err: PackageError = cmd_err.into();
        assert!(matches!(err, PackageError::Command(_)));
    }

    #[test]
    fn test_validation_error_display() {
        let err = ValidationError::ProjectValidation {
            reason: "bad project".to_string(),
        };
        assert!(err.to_string().contains("bad project"));
    }
}

//! Error handling for the Pacor cross-platform packaging tool.
//!
//! This module provides a comprehensive error system with fine-grained error types
//! to improve debugging and error recovery capabilities across the application.

mod build;
mod command;
mod config;
mod platform;
mod project;
mod validate;

pub(crate) use self::build::BuildError;
pub(crate) use self::command::CommandError;
pub(crate) use self::config::ConfigurationError;
pub(crate) use self::platform::PlatformError;
pub(crate) use self::project::ProjectError;
pub(crate) use self::validate::ValidationError;

/// Result type alias for Pacor operations.
pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

/// Main error enum for all Pacor operations.
///
/// This enum provides fine-grained error categorization to improve error handling,
/// debugging, and user experience. Each category contains specific error variants
/// that provide detailed context about what went wrong.
#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

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
        let err = BuildError::FlutterBuild {
            output: "error message".to_string(),
            platform: "linux".to_string(),
        };
        assert!(err.to_string().contains("error message"));
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
        let err: Error = ProjectError::FlutterNotInstalled.into();
        assert!(matches!(err, Error::Project(_)));
    }

    #[test]
    fn test_package_error_from_command() {
        let cmd_err = CommandError::Failed {
            command: OsString::from("test"),
            exit_info: "exit code 1".to_string(),
        };
        let err: Error = cmd_err.into();
        assert!(matches!(err, Error::Command(_)));
    }

    #[test]
    fn test_validation_error_display() {
        let err = ValidationError::ProjectValidation {
            reason: "bad project".to_string(),
        };
        assert!(err.to_string().contains("bad project"));
    }
}

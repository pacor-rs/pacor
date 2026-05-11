use std::path::PathBuf;

/// Validation errors for project and environment checks.
#[derive(thiserror::Error, Debug)]
pub(crate) enum ValidationError {
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

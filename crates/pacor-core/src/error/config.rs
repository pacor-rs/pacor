use std::path::PathBuf;

/// Configuration-related errors with specific context.
#[derive(thiserror::Error, Debug)]
pub enum ConfigurationError {
    #[error("Platform not enabled: {platform}")]
    PlatformNotEnabled { platform: String },

    #[error("Configuration file not found at {}", path.display())]
    FileNotFound { path: PathBuf },

    /// The default `pacor.toml` in the Flutter project root is missing.
    ///
    /// Separate from [`Self::FileNotFound`] so the error can point at
    /// `pacor init`; an explicit `--config` path may live anywhere and is not
    /// necessarily created by that command.
    #[error(
        "Configuration file not found at {}. Run `pacor init` in the Flutter project root to \
         create it, or pass `--config <path>`",
        path.display()
    )]
    DefaultConfigNotFound { path: PathBuf },

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

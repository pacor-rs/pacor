use std::path::PathBuf;

/// Platform-specific errors with detailed context.
#[derive(thiserror::Error, Debug)]
pub enum PlatformError {
    #[error("Unsupported platform: {platform}")]
    UnsupportedPlatform { platform: String },

    #[error("Platform tool not found: {tool} (required for {platform})")]
    ToolNotFound { tool: String, platform: String },

    #[error("Platform build failed for {platform}: {reason}")]
    BuildFailed { platform: String, reason: String },

    #[error("File not found: {}", path.display())]
    FileNotFound { path: PathBuf },

    #[error("Invalid platform configuration for {platform}: {reason}")]
    InvalidConfiguration { platform: String, reason: String },

    #[error("Invalid XCConfig format at line {line}: {content}")]
    InvalidXcconfigFormat { line: usize, content: String },
}

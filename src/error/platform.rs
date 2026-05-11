/// Platform-specific errors with detailed context.
#[derive(thiserror::Error, Debug)]
pub(crate) enum PlatformError {
    #[error("Unsupported platform: {platform}")]
    UnsupportedPlatform { platform: String },

    #[error("Platform tool not found: {tool} (required for {platform})")]
    ToolNotFound { tool: String, platform: String },

    #[cfg(windows)]
    #[error("File not found: {}", path.display())]
    FileNotFound { path: PathBuf },

    #[cfg(windows)]
    #[error("Invalid platform configuration for {platform}: {reason}")]
    InvalidConfiguration { platform: String, reason: String },

    #[cfg(target_os = "macos")]
    #[error("Invalid XCConfig format at line {line}: {content}")]
    InvalidXcconfigFormat { line: usize, content: String },
}

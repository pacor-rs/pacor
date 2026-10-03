//! Platform-agnostic build metadata shared by every packaging backend.

use std::path::PathBuf;

/// A produced installer/package together with its checksum file.
pub struct BuildArtifact {
    file: PathBuf,
    sha1: PathBuf,
}

impl BuildArtifact {
    pub fn new(file: PathBuf, sha1: PathBuf) -> Self {
        Self { file, sha1 }
    }

    pub fn file(&self) -> &PathBuf {
        &self.file
    }

    pub fn sha1(&self) -> &PathBuf {
        &self.sha1
    }
}

/// A packaging target.
///
/// Unlike the previous host-gated enum, every variant is always available so a
/// single configuration can refer to any platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Android,
    Linux,
    MacOS,
    Windows,
}

impl Platform {
    /// Every supported target, in a stable order.
    pub const ALL: [Platform; 4] = [
        Platform::Linux,
        Platform::MacOS,
        Platform::Windows,
        Platform::Android,
    ];

    /// Canonical lowercase name used in configuration, CLI and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Android => "android",
            Platform::Linux => "linux",
            Platform::MacOS => "macos",
            Platform::Windows => "windows",
        }
    }

    /// Target matching the host the binary is running on, if supported.
    pub fn host() -> Option<Platform> {
        match std::env::consts::OS {
            "linux" => Some(Platform::Linux),
            "macos" => Some(Platform::MacOS),
            "windows" => Some(Platform::Windows),
            _ => None,
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "linux" => Ok(Platform::Linux),
            "macos" | "darwin" | "mac" => Ok(Platform::MacOS),
            "windows" | "win" => Ok(Platform::Windows),
            "android" => Ok(Platform::Android),
            other => Err(format!(
                "unsupported target `{other}` (expected one of: linux, macos, windows, android)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_artifact_new() {
        let artifact = BuildArtifact::new(
            PathBuf::from("output/app.exe"),
            PathBuf::from("output/app.exe.sha1"),
        );
        assert_eq!(artifact.file(), &PathBuf::from("output/app.exe"));
        assert_eq!(artifact.sha1(), &PathBuf::from("output/app.exe.sha1"));
    }
}

use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

pub(crate) mod android;

pub(crate) struct BuildArtifact {
    file: PathBuf,
    sha1: PathBuf,
}

impl BuildArtifact {
    pub(crate) fn new(file: PathBuf, sha1: PathBuf) -> Self {
        Self { file, sha1 }
    }

    pub(crate) fn file(&self) -> &PathBuf {
        &self.file
    }

    pub(crate) fn sha1(&self) -> &PathBuf {
        &self.sha1
    }
}

pub(crate) enum Platform {
    Android,
    #[cfg(target_os = "linux")]
    Linux,
    #[cfg(target_os = "macos")]
    MacOS,
    #[cfg(target_os = "windows")]
    Windows,
}

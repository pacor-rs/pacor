use std::fmt::Display;

use anyhow::Result;

use crate::error::PackageError;

/// Only 64-bit architectures are mainstream for modern operating systems.
/// 32-bit support has been deprecated, and this project does not maintain 32-bit versions.
#[derive(Debug, Clone)]
pub(crate) enum Arch {
    X64,
    ARM64,
}

impl Arch {
    pub(crate) fn current() -> Result<Arch, PackageError> {
        let arch = std::env::consts::ARCH;
        match arch {
            "x86_64" => Ok(Arch::X64),
            "aarch64" => Ok(Arch::ARM64),
            _ => Err(PackageError::Architecture(arch.to_string())),
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Arch::X64 => "x64",
            Arch::ARM64 => "arm64",
        }
    }
}

impl AsRef<str> for Arch {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

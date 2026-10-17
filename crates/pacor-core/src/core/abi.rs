use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Represents the target Application Binary Interface (ABI) for Flutter Android builds.
///
/// This enum defines the supported CPU architectures for Flutter Android applications
/// and provides methods to convert between different naming conventions used in the
/// Flutter build process.
///
/// ## Note:
///
/// Cannot build release mode for x86 ABI.
/// For more information see https://flutter.dev/to/android-supported-architectures
///
/// The ordering (`Arm` < `Arm64` < `X64`) is what makes `BTreeSet<TargetAbi>`
/// and every list derived from it stable across runs.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub enum TargetAbi {
    /// 32-bit ARM architecture (ARMv7) - used for older Android devices.
    #[serde(rename = "armeabi-v7a", alias = "arm")]
    Arm,
    /// 64-bit ARM architecture (ARMv8-A) - modern Android standard.
    #[serde(rename = "arm64-v8a", alias = "arm64")]
    Arm64,
    /// 64-bit x86 architecture - for x86_64 Android emulators.
    #[serde(rename = "x86_64", alias = "x64")]
    X64,
}

impl TargetAbi {
    /// Returns the ABI string used for Flutter APK file naming and lib directory structure.
    ///
    /// This follows the Android NDK ABI naming convention that Flutter uses for:
    /// - APK filename suffixes (e.g., `app-armeabi-v7a-release.apk`)
    /// - Native library directory names in APKs (e.g., `lib/armeabi-v7a/`)
    ///
    /// # Examples
    ///
    /// ```text
    /// TargetAbi::Arm.as_apk_abi()     == "armeabi-v7a"
    /// TargetAbi::Arm64.as_apk_abi()   == "arm64-v8a"
    /// assert_eq!(TargetAbi::Arm64.as_apk_abi(), "arm64-v8a");
    /// ```
    pub fn as_apk_abi(&self) -> &'static str {
        match self {
            Self::Arm => "armeabi-v7a",
            Self::Arm64 => "arm64-v8a",
            Self::X64 => "x86_64",
        }
    }

    /// Returns the target platform string used with `flutter build apk` command.
    ///
    /// This string is passed to the `--target-platform` flag when building split APKs
    /// with Flutter CLI. For example:
    /// ```bash
    /// flutter build apk --target-platform=android-arm64
    /// ```
    ///
    /// # Examples
    ///
    /// ```text
    /// TargetAbi::Arm.as_target_platform()   == "android-arm"
    /// TargetAbi::X64.as_target_platform()   == "android-x64"
    /// ```
    pub fn as_target_platform(&self) -> &'static str {
        match self {
            Self::Arm => "android-arm",
            Self::Arm64 => "android-arm64",
            Self::X64 => "android-x64",
        }
    }
}

impl Display for TargetAbi {
    /// Formats the TargetAbi as its APK ABI representation for Flutter builds.
    ///
    /// This implementation delegates to `as_apk_abi()` to provide the standard
    /// Android ABI string used in Flutter APK naming and packaging.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_apk_abi())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_apk_abi() {
        assert_eq!(TargetAbi::Arm.as_apk_abi(), "armeabi-v7a");
        assert_eq!(TargetAbi::Arm64.as_apk_abi(), "arm64-v8a");
        assert_eq!(TargetAbi::X64.as_apk_abi(), "x86_64");
    }

    #[test]
    fn test_as_target_platform() {
        assert_eq!(TargetAbi::Arm.as_target_platform(), "android-arm");
        assert_eq!(TargetAbi::Arm64.as_target_platform(), "android-arm64");
        assert_eq!(TargetAbi::X64.as_target_platform(), "android-x64");
    }

    #[test]
    fn test_display_impl() {
        assert_eq!(format!("{}", TargetAbi::Arm), "armeabi-v7a");
        assert_eq!(format!("{}", TargetAbi::Arm64), "arm64-v8a");
        assert_eq!(format!("{}", TargetAbi::X64), "x86_64");
    }
}

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::abi::TargetAbi;

/// Android output formats.
///
/// An APK is what a device installs directly (websites, MDM, self-update); an
/// Android App Bundle is a publishing format for Google Play, which splits it
/// per device and re-signs it. They are complementary, and a project that only
/// ships APKs should not pay for building a bundle — hence `["apk"]` by default.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AndroidPackageFormat {
    /// Per-ABI APK, installed directly.
    Apk,
    /// Android App Bundle (`.aab`), uploaded to Google Play.
    Aab,
}

impl AndroidPackageFormat {
    /// Every format, in a stable order.
    pub const ALL: [Self; 2] = [Self::Apk, Self::Aab];

    /// Lowercase name used in configuration, on the command line and in messages.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Apk => "apk",
            Self::Aab => "aab",
        }
    }
}

impl std::str::FromStr for AndroidPackageFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "apk" => Ok(Self::Apk),
            "aab" => Ok(Self::Aab),
            other => Err(format!(
                "unsupported Android format `{other}` (expected one of: apk, aab)"
            )),
        }
    }
}

impl std::fmt::Display for AndroidPackageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Android platform configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AndroidConfig {
    #[serde(default = "super::default_enabled")]
    pub enabled: bool,

    /// ABIs to build.
    ///
    /// A `BTreeSet` (rather than a `HashSet`) keeps the generated
    /// `--target-platform` list, the artifact order and every report stable
    /// across runs. An explicitly empty list is a configuration error: it used
    /// to silently fall back to `arm64-v8a` while Flutter built every ABI.
    ///
    /// For an APK this is the set of files produced; for an App Bundle it is the
    /// set of ABIs inside the single `.aab`.
    pub target_abi: BTreeSet<TargetAbi>,

    /// Output formats to produce, e.g. `["apk", "aab"]`. Defaults to `["apk"]`.
    #[serde(rename = "formats", alias = "package_formats")]
    pub formats: BTreeSet<AndroidPackageFormat>,

    /// Gradle product flavor, passed through as `flutter build … --flavor`.
    ///
    /// The flavor becomes part of the APK and AAB file names Flutter produces,
    /// which the artifact lookups account for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<String>,

    /// Upload/release keystore. Setting this enables signing of every format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keystore_file: Option<String>,

    /// Keystore password. Prefer setting `PACOR_ANDROID_KEYSTORE_PASSWORD` env var.
    #[serde(default, skip_serializing)]
    keystore_password: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_alias: Option<String>,

    /// Key password. Prefer setting `PACOR_ANDROID_KEY_PASSWORD` env var.
    #[serde(default, skip_serializing)]
    key_password: Option<String>,

    /// Explicit `apksigner` path, used for APKs. A lookup hint only: it does not
    /// enable signing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_tool_path: Option<PathBuf>,

    /// Explicit `jarsigner` path, used for App Bundles. A lookup hint only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jarsigner_path: Option<PathBuf>,
}

impl AndroidConfig {
    /// Environment variable that overrides the keystore password.
    pub const KEYSTORE_PASSWORD_ENV: &'static str = "PACOR_ANDROID_KEYSTORE_PASSWORD";

    /// Environment variable that overrides the key password.
    pub const KEY_PASSWORD_ENV: &'static str = "PACOR_ANDROID_KEY_PASSWORD";

    pub fn keystore_password(&self) -> Option<String> {
        std::env::var(Self::KEYSTORE_PASSWORD_ENV)
            .ok()
            .or_else(|| self.keystore_password.clone())
    }

    pub fn key_password(&self) -> Option<String> {
        std::env::var(Self::KEY_PASSWORD_ENV)
            .ok()
            .or_else(|| self.key_password.clone())
    }

    /// Whether signing is configured.
    ///
    /// The keystore is the switch; the two tool paths only tell pacor where the
    /// signers are.
    pub fn should_sign_apk(&self) -> bool {
        self.keystore_file.is_some()
    }

    /// The configured keystore, resolved against the project root.
    ///
    /// `keystore_file` is written relative to the project (as the template
    /// suggests: `android/app/upload-keystore.jks`), but pacor may run from a
    /// subdirectory, so the path is resolved explicitly instead of relying on
    /// the process working directory.
    pub fn keystore_path(&self, project_root: &Path) -> Option<PathBuf> {
        self.keystore_file.as_deref().map(|file| {
            let path = Path::new(file);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_root.join(path)
            }
        })
    }

    /// Gradle flavor, ignoring blank values.
    pub fn flavor(&self) -> Option<&str> {
        self.flavor
            .as_deref()
            .map(str::trim)
            .filter(|flavor| !flavor.is_empty())
    }

    pub fn formats(&self) -> &BTreeSet<AndroidPackageFormat> {
        &self.formats
    }

    /// Whether per-ABI APKs are part of this configuration.
    pub fn builds_apk(&self) -> bool {
        self.formats.contains(&AndroidPackageFormat::Apk)
    }

    /// Whether an Android App Bundle is part of this configuration.
    pub fn builds_bundle(&self) -> bool {
        self.formats.contains(&AndroidPackageFormat::Aab)
    }

    /// Formats as a human-readable list, for logs and error messages.
    pub fn format_list(&self) -> String {
        self.formats
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Default for AndroidConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            target_abi: BTreeSet::from([TargetAbi::Arm64]),
            formats: BTreeSet::from([AndroidPackageFormat::Apk]),
            flavor: None,
            keystore_file: None,
            keystore_password: None,
            key_alias: None,
            key_password: None,
            signing_tool_path: None,
            jarsigner_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_round_trip() {
        for format in AndroidPackageFormat::ALL {
            assert_eq!(
                format.as_str().parse::<AndroidPackageFormat>().unwrap(),
                format
            );
            assert_eq!(format.to_string().as_str(), format.as_str());
        }

        let error = "apks".parse::<AndroidPackageFormat>().unwrap_err();
        assert!(error.contains("apks"), "{error}");
        assert!(error.contains("apk, aab"), "{error}");
    }

    /// Some projects only ship APKs, so a bundle has to be opt-in.
    #[test]
    fn the_default_configuration_builds_apks_only() {
        let config = AndroidConfig::default();

        assert!(config.builds_apk());
        assert!(!config.builds_bundle());
        assert_eq!(config.format_list(), "apk");
    }

    #[test]
    fn formats_are_deduplicated_and_ordered() {
        let config: AndroidConfig = toml::from_str(r#"formats = ["aab", "apk", "aab"]"#).unwrap();

        assert_eq!(
            config.formats().iter().copied().collect::<Vec<_>>(),
            vec![AndroidPackageFormat::Apk, AndroidPackageFormat::Aab]
        );
        assert_eq!(config.format_list(), "apk, aab");
        assert!(config.builds_apk() && config.builds_bundle());
    }

    #[test]
    fn a_missing_format_list_defaults_to_apk() {
        let config: AndroidConfig = toml::from_str("enabled = true").unwrap();

        assert!(config.builds_apk());
        assert!(!config.builds_bundle());
    }

    #[test]
    fn an_explicitly_empty_format_list_stays_empty() {
        let config: AndroidConfig = toml::from_str("formats = []").unwrap();

        assert!(config.formats().is_empty());
        assert!(!config.builds_apk() && !config.builds_bundle());
    }

    #[test]
    fn the_package_formats_alias_is_accepted() {
        let config: AndroidConfig = toml::from_str(r#"package_formats = ["aab"]"#).unwrap();

        assert!(config.builds_bundle());
        assert!(!config.builds_apk());
    }

    #[test]
    fn a_missing_abi_list_defaults_to_arm64_only() {
        let config: AndroidConfig = toml::from_str("enabled = true").unwrap();

        assert_eq!(
            config.target_abi,
            BTreeSet::from([TargetAbi::Arm64]),
            "an absent key keeps the default"
        );
    }

    /// `[]` must stay empty instead of falling back, so the builder can reject it.
    #[test]
    fn an_explicitly_empty_abi_list_stays_empty() {
        let config: AndroidConfig = toml::from_str("target_abi = []").unwrap();

        assert!(config.target_abi.is_empty());
    }

    #[test]
    fn abis_are_deduplicated_and_ordered() {
        let config: AndroidConfig =
            toml::from_str(r#"target_abi = ["x86_64", "arm64-v8a", "arm64", "armeabi-v7a"]"#)
                .unwrap();

        assert_eq!(
            config.target_abi.iter().copied().collect::<Vec<_>>(),
            vec![TargetAbi::Arm, TargetAbi::Arm64, TargetAbi::X64],
            "aliases deduplicate and the order is fixed"
        );
    }

    #[test]
    fn relative_keystores_resolve_against_the_project_root() {
        let config: AndroidConfig =
            toml::from_str(r#"keystore_file = "android/app/upload.jks""#).unwrap();

        assert_eq!(
            config.keystore_path(Path::new("/project")),
            Some(PathBuf::from("/project/android/app/upload.jks"))
        );

        let absolute: AndroidConfig =
            toml::from_str(r#"keystore_file = "/keys/upload.jks""#).unwrap();
        assert_eq!(
            absolute.keystore_path(Path::new("/project")),
            Some(PathBuf::from("/keys/upload.jks"))
        );
    }

    #[test]
    fn a_keystore_switches_signing_on_and_a_tool_path_does_not() {
        let mut config = AndroidConfig::default();
        assert!(!config.should_sign_apk());

        config.signing_tool_path = Some(PathBuf::from("/sdk/apksigner"));
        config.jarsigner_path = Some(PathBuf::from("/jdk/bin/jarsigner"));
        assert!(
            !config.should_sign_apk(),
            "a tool path is a lookup hint, not a switch"
        );

        config.keystore_file = Some("upload.jks".to_string());
        assert!(config.should_sign_apk());
    }

    #[test]
    fn blank_flavors_are_ignored() {
        let config: AndroidConfig = toml::from_str(r#"flavor = "  ""#).unwrap();

        assert_eq!(config.flavor(), None);
    }
}

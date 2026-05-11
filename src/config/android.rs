use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::platforms::android::TargetAbi;

/// Android platform configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AndroidConfig {
    pub(crate) enabled: bool,
    pub(crate) target_abi: HashSet<TargetAbi>,
    pub(crate) keystore_file: Option<String>,
    /// Keystore password. Prefer setting `PACOR_ANDROID_KEYSTORE_PASSWORD` env var.
    #[serde(skip_serializing)]
    keystore_password: Option<String>,
    pub(crate) key_alias: Option<String>,
    /// Key password. Prefer setting `PACOR_ANDROID_KEY_PASSWORD` env var.
    #[serde(skip_serializing)]
    key_password: Option<String>,
}

impl AndroidConfig {
    pub(crate) fn keystore_password(&self) -> Option<String> {
        std::env::var("PACOR_ANDROID_KEYSTORE_PASSWORD")
            .ok()
            .or_else(|| self.keystore_password.clone())
    }

    pub(crate) fn key_password(&self) -> Option<String> {
        std::env::var("PACOR_ANDROID_KEY_PASSWORD")
            .ok()
            .or_else(|| self.key_password.clone())
    }
}

impl Default for AndroidConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            target_abi: HashSet::from([TargetAbi::Arm64]),
            keystore_file: None,
            keystore_password: None,
            key_alias: None,
            key_password: None,
        }
    }
}

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::core::abi::TargetAbi;

/// Android platform configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AndroidConfig {
    #[serde(default = "super::default_enabled")]
    pub enabled: bool,
    pub target_abi: HashSet<TargetAbi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keystore_file: Option<String>,
    /// Keystore password. Prefer setting `PACOR_ANDROID_KEYSTORE_PASSWORD` env var.
    #[serde(skip_serializing)]
    keystore_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_alias: Option<String>,
    /// Key password. Prefer setting `PACOR_ANDROID_KEY_PASSWORD` env var.
    #[serde(skip_serializing)]
    key_password: Option<String>,
}

impl AndroidConfig {
    pub fn keystore_password(&self) -> Option<String> {
        std::env::var("PACOR_ANDROID_KEYSTORE_PASSWORD")
            .ok()
            .or_else(|| self.keystore_password.clone())
    }

    pub fn key_password(&self) -> Option<String> {
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

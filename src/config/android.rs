use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::platforms::android::TargetAbi;

/// Android platform configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AndroidConfig {
    pub(crate) enabled: bool,
    pub(crate) target_abi: HashSet<TargetAbi>,
    pub(crate) keystore_file: Option<String>,
    pub(crate) keystore_password: Option<String>,
    pub(crate) key_alias: Option<String>,
    pub(crate) key_password: Option<String>,
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

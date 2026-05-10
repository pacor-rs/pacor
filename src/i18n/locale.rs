use std::fmt::{Display, Formatter, Result};

use serde::{Deserialize, Serialize};

/// Locale identifier
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) enum Locale {
    #[default]
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "en-GB")]
    EnGb,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "ja-JP")]
    JaJp,
}

impl Display for Locale {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let s = match self {
            Locale::EnUs => "en-US",
            Locale::EnGb => "en-GB",
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
            Locale::JaJp => "ja-JP",
        };

        write!(f, "{}", s)
    }
}

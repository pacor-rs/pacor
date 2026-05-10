use serde::{Deserialize, Serialize};

/// Build options
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct BuildOptions {
    /// Flutter build mode (release/debug/profile)
    pub(crate) flutter_build_mode: String,

    /// Output directory
    pub(crate) output_dir: String,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            flutter_build_mode: "release".to_string(),
            output_dir: "dist".to_string(),
        }
    }
}

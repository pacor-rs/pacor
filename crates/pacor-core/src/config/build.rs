use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlutterBuildMode {
    #[default]
    Release,
    Debug,
    Profile,
}

impl FlutterBuildMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlutterBuildMode::Release => "release",
            FlutterBuildMode::Debug => "debug",
            FlutterBuildMode::Profile => "profile",
        }
    }
}

impl std::str::FromStr for FlutterBuildMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "release" => Ok(Self::Release),
            "debug" => Ok(Self::Debug),
            "profile" => Ok(Self::Profile),
            other => Err(format!(
                "invalid build mode `{other}` (expected one of: release, debug, profile)"
            )),
        }
    }
}

impl std::fmt::Display for FlutterBuildMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Build options
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BuildOptions {
    pub flutter_build_mode: FlutterBuildMode,
    pub output_dir: String,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            flutter_build_mode: FlutterBuildMode::Release,
            output_dir: "dist".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_build_mode_is_release() {
        assert_eq!(FlutterBuildMode::default(), FlutterBuildMode::Release);
    }

    #[test]
    fn test_build_mode_as_str() {
        assert_eq!(FlutterBuildMode::Release.as_str(), "release");
        assert_eq!(FlutterBuildMode::Debug.as_str(), "debug");
        assert_eq!(FlutterBuildMode::Profile.as_str(), "profile");
    }

    #[test]
    fn test_build_mode_display() {
        assert_eq!(format!("{}", FlutterBuildMode::Release), "release");
    }

    #[test]
    fn test_build_mode_serde() {
        let toml_str = toml::to_string(&BuildOptions {
            flutter_build_mode: FlutterBuildMode::Debug,
            output_dir: "dist".to_string(),
        })
        .unwrap();
        assert!(toml_str.contains("debug"));
        let opts: BuildOptions = toml::from_str(&toml_str).unwrap();
        assert_eq!(opts.flutter_build_mode, FlutterBuildMode::Debug);
    }

    #[test]
    fn test_build_options_default() {
        let opts = BuildOptions::default();
        assert_eq!(opts.flutter_build_mode, FlutterBuildMode::Release);
        assert_eq!(opts.output_dir, "dist");
    }
}

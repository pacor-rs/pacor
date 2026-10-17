//! `flutter build apk` / `flutter build appbundle` invocation for Android.

use std::{ffi::OsString, path::Path};

use anyhow::Result;

use pacor_core::{
    config::AndroidConfig,
    utils::command::{CommandOutputMode, macros::command_args},
};

/// Directory (relative to the project root) that split debug symbols go to.
///
/// Release APKs and bundles ship without their Dart AOT symbols
/// (`--split-debug-info` keeps them out so the artifact is smaller); the symbols
/// are what `flutter symbolize` needs to turn a stack trace from a released
/// build back into method names. They stay next to the project until
/// [`crate::AndroidBuilder::clean`] removes them, which is what makes
/// `--skip-cleanup` useful.
pub(crate) const SYMBOLS_DIR: &str = "tmp";

/// Builds the release APKs for every configured ABI in one Flutter invocation.
///
/// Android always builds release: the build mode is part of the file name
/// rather than of the output directory, so `[build] flutter_build_mode` does not
/// apply here.
pub(crate) fn build_apk(
    project_root: &Path,
    config: &AndroidConfig,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    let mut args = shared_args("apk", config);
    // Flag order is irrelevant to Flutter; keeping this here makes the
    // APK-only nature of `--split-per-abi` visible.
    args.push("--split-per-abi".into());

    info!(abis = %target_platforms(config), "Building split release APKs");
    run(args, project_root, command_output_mode)
}

/// Builds the Android App Bundle, which Google Play splits per device.
pub(crate) fn build_app_bundle(
    project_root: &Path,
    config: &AndroidConfig,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    let args = shared_args("appbundle", config);

    info!(
        abis = %target_platforms(config),
        "Building a release Android App Bundle"
    );
    run(args, project_root, command_output_mode)
}

/// Arguments both Android builds share: mode, symbols and the ABI restriction.
fn shared_args(command: &str, config: &AndroidConfig) -> Vec<OsString> {
    let mut args = command_args![
        command,
        "--release",
        format!("--split-debug-info={SYMBOLS_DIR}/")
    ];

    if let Some(flavor) = config.flavor() {
        info!(flavor, "Building the configured Gradle flavor");
        args.push("--flavor".into());
        args.push(flavor.into());
    }

    // `target_abi` is never empty here (the builder rejects that), and a
    // `BTreeSet` gives the same argument order on every run. For a bundle this
    // restricts which ABIs the single `.aab` contains.
    debug!(platforms = %target_platforms(config), "Restricting the build to the configured ABIs");
    args.push("--target-platform".into());
    args.push(target_platforms(config).into());

    args
}

fn run(
    args: Vec<OsString>,
    project_root: &Path,
    command_output_mode: &CommandOutputMode,
) -> Result<()> {
    pacor_core::packaging::flutter_build(&args, project_root, command_output_mode, "android")?;

    Ok(())
}

/// Comma-separated `--target-platform` value for the configured ABIs.
pub(crate) fn target_platforms(config: &AndroidConfig) -> String {
    config
        .target_abi
        .iter()
        .map(|abi| abi.as_target_platform())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacor_core::core::abi::TargetAbi;

    #[test]
    fn target_platforms_follow_the_configured_order() {
        let config: AndroidConfig =
            toml::from_str(r#"target_abi = ["x86_64", "arm64-v8a", "armeabi-v7a"]"#).unwrap();

        assert_eq!(
            target_platforms(&config),
            "android-arm,android-arm64,android-x64",
            "the order is fixed, not the order of the TOML array"
        );
    }

    #[test]
    fn a_single_abi_needs_no_separator() {
        let config: AndroidConfig = toml::from_str(r#"target_abi = ["arm64-v8a"]"#).unwrap();

        assert_eq!(target_platforms(&config), "android-arm64");
        assert_eq!(config.target_abi.len(), 1);
        assert!(config.target_abi.contains(&TargetAbi::Arm64));
    }

    #[test]
    fn an_apk_build_splits_per_abi_and_a_bundle_does_not() {
        let config = AndroidConfig::default();

        let apk = shared_args("apk", &config);
        let bundle = shared_args("appbundle", &config);

        assert_eq!(apk[0], "apk");
        assert_eq!(bundle[0], "appbundle");
        assert!(apk.contains(&"--split-debug-info=tmp/".into()));
        assert!(bundle.contains(&"--split-debug-info=tmp/".into()));
        assert_eq!(
            apk.iter().filter(|arg| *arg == "--split-per-abi").count(),
            0
        );
    }

    #[test]
    fn the_flavor_is_passed_through() {
        let config: AndroidConfig = toml::from_str(r#"flavor = "prod""#).unwrap();

        let args = shared_args("appbundle", &config);
        let flavor = args.iter().position(|arg| arg == "--flavor").unwrap();

        assert_eq!(args[flavor + 1], "prod");
    }
}

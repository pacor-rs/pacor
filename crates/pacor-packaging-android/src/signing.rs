//! APK signing with `apksigner`, plus the signer check that catches
//! debug-signed release artifacts.
//!
//! Signing is driven by `platforms.android`: configuring a keystore switches it
//! on, and every input it needs is validated before the build starts, because
//! silently shipping an APK that is not signed with the configured key is worse
//! than failing.
//!
//! An APK is signed with the APK signature schemes, which is what `apksigner`
//! implements; an Android App Bundle is a JAR and goes through
//! [`super::jarsigner`] instead.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};

use pacor_core::{
    config::AndroidConfig,
    utils::command::{CommandOutputMode, execute_command, find_command_path, macros::command_args},
};

use super::{
    certificate::{Signer, first_line},
    jdk,
};

/// How `apksigner` is named on this host.
const APKSIGNER: &str = "apksigner";

/// Resolved signing configuration: a tool plus the keystore it uses.
pub(super) struct ApkSigner {
    tool: PathBuf,

    keystore: PathBuf,

    alias: String,

    /// Never logged or formatted by this type.
    keystore_password: String,

    /// Never logged or formatted by this type.
    key_password: String,
}

impl ApkSigner {
    /// Resolves the signing tool and validates the keystore configuration.
    ///
    /// # Errors
    /// Fails when the keystore is missing or absent from disk, the alias or
    /// either password is unset, or no `apksigner` can be found. Signing is
    /// configured explicitly, so an unsigned build would be a silent
    /// downgrade — exactly the behaviour this replaced.
    pub(super) fn resolve(config: &AndroidConfig, project_root: &Path) -> Result<Self> {
        let keystore = config
            .keystore_path(project_root)
            .ok_or_else(|| anyhow!("`platforms.android.keystore_file` must be set to sign APKs"))?;

        if !keystore.is_file() {
            bail!(
                "the configured keystore {} does not exist (paths are resolved relative to the \
                 project root)",
                keystore.display()
            );
        }

        let alias = config
            .key_alias
            .as_deref()
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .ok_or_else(|| anyhow!("set `platforms.android.key_alias` to sign APKs"))?
            .to_string();

        let keystore_password = config.keystore_password().ok_or_else(|| {
            anyhow!(
                "set `PACOR_ANDROID_KEYSTORE_PASSWORD` (or `platforms.android.keystore_password`) \
                 to sign APKs"
            )
        })?;

        let key_password = config.key_password().ok_or_else(|| {
            anyhow!(
                "set `PACOR_ANDROID_KEY_PASSWORD` (or `platforms.android.key_password`) to sign APKs"
            )
        })?;

        let tool = resolve_apksigner(config, project_root)?;

        debug!(
            tool = %tool.display(),
            keystore = %keystore.display(),
            alias = %alias,
            "Resolved the APK signing toolchain"
        );

        Ok(Self {
            tool,
            keystore,
            alias,
            keystore_password,
            key_password,
        })
    }

    /// Signs one APK in place.
    pub(super) fn sign(&self, apk: &Path, command_output_mode: &CommandOutputMode) -> Result<()> {
        debug!(apk = %apk.display(), "Signing APK");

        // `--ks-pass`/`--key-pass` take `env:<name>`, `pass:<literal>`, `file:`
        // or `stdin`. Passwords that came from the environment are passed by
        // name so they never reach the process list.
        let keystore_pass = password_spec(
            AndroidConfig::KEYSTORE_PASSWORD_ENV,
            &self.keystore_password,
        );
        let key_pass = password_spec(AndroidConfig::KEY_PASSWORD_ENV, &self.key_password);

        // v1/v2/v3 scheme selection is left to apksigner, which decides from the
        // APK's own minSdkVersion/targetSdkVersion.
        //
        // v4 is the exception: it is enabled by default and makes apksigner write
        // a second file (`<apk>.idsig`, for `adb install --incremental`) next to
        // the APK. That file is neither reported nor checksummed by pacor, so the
        // output directory would silently contain more than pacor says it does.
        let mut args = command_args!(
            capacity = 14,
            "sign",
            "--ks",
            &self.keystore,
            "--ks-key-alias",
            self.alias.as_str(),
            "--ks-pass",
            keystore_pass,
            "--key-pass",
            key_pass,
            "--v4-signing-enabled",
            "false",
        );
        args.push(apk.as_os_str().into());

        let java_home = java_home_for_apksigner();
        execute_command(
            &self.tool,
            &args,
            None,
            java_home.as_ref().map(std::slice::from_ref),
            command_output_mode,
        )
        .with_context(|| {
            format!(
                "failed to sign {}: re-run with `--command-output normal` to see apksigner's own \
                 diagnostics",
                apk.display()
            )
        })?;

        info!(apk = %apk.display(), "APK signed");
        Ok(())
    }
}

/// `JAVA_HOME` for the `apksigner` launcher, when the environment has none.
///
/// `apksigner.bat` is a shell wrapper around `java`: without `JAVA_HOME` (or a
/// `java` on `PATH`) it exits with
/// `ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH`.
/// Flutter resolves Android Studio's JBR for its own Gradle runs, but a shell
/// that only runs pacor usually has no JDK configured, so pacor passes the JDK it
/// finds instead of failing.
fn java_home_for_apksigner() -> Option<(&'static str, std::ffi::OsString)> {
    if env::var_os("JAVA_HOME").is_some_and(|value| !value.is_empty()) {
        return None;
    }

    let home = jdk::home_for_tools()?;
    debug!(
        java_home = %home.display(),
        "No JAVA_HOME in the environment; passing the JDK found on this machine to apksigner"
    );

    Some(("JAVA_HOME", home.into_os_string()))
}

/// Inspects the certificate of an already built APK, best effort.
///
/// This runs only when pacor did not sign the APK itself, where the signature is
/// whatever the project's Gradle configuration produced — Flutter's own template
/// points release builds at the debug keystore, so this is the difference between
/// a publishable artifact and one Google Play rejects.
pub(super) fn inspect_apk(config: &AndroidConfig, project_root: &Path, apk: &Path) -> Signer {
    let Some(tool) = find_apksigner(config, project_root) else {
        debug!("apksigner was not found; skipping the APK signer check");
        return Signer::Unknown;
    };

    let args = command_args!("verify", "--print-certs", apk);
    let mut command = Command::new(&tool);
    command.args(&args);
    if let Some((key, value)) = java_home_for_apksigner() {
        command.env(key, value);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            debug!(tool = %tool.display(), %error, "Failed to run apksigner verify");
            return Signer::Unknown;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(subject) = parse_signer(&stdout).or_else(|| parse_signer(&stderr)) {
        return Signer::from_subject(subject);
    }

    if output.status.success() {
        return Signer::Unknown;
    }

    // `apksigner` reports a missing JDK on stdout, not stderr, so both streams
    // have to be considered before falling back to a generic message.
    Signer::Unverified(
        first_line(&stderr)
            .or_else(|| first_line(&stdout))
            .unwrap_or_else(|| "apksigner verify failed".to_string()),
    )
}

/// Finds `apksigner`: explicit configuration, then `PATH`, then the Android SDK.
fn resolve_apksigner(config: &AndroidConfig, project_root: &Path) -> Result<PathBuf> {
    if let Some(configured) = config.signing_tool_path.as_ref() {
        if !configured.is_file() {
            bail!(
                "the configured signing tool {} does not exist",
                configured.display()
            );
        }
        return Ok(configured.clone());
    }

    if let Some(found) = find_apksigner(config, project_root) {
        return Ok(found);
    }

    bail!(
        "no signing tool found: install the Android SDK build-tools, put `apksigner` on `PATH`, or \
         set `platforms.android.signing_tool_path`"
    )
}

/// Same lookup as [`resolve_apksigner`], but `None` instead of an error.
fn find_apksigner(config: &AndroidConfig, project_root: &Path) -> Option<PathBuf> {
    if let Some(configured) = config
        .signing_tool_path
        .as_ref()
        .filter(|tool| tool.is_file())
    {
        return Some(configured.clone());
    }

    if let Ok(Some(found)) = find_command_path(APKSIGNER) {
        return Some(found);
    }

    find_apksigner_in_android_sdk(project_root)
}

/// Looks for `apksigner` in the Android SDK's `build-tools`, newest first.
fn find_apksigner_in_android_sdk(project_root: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    for root in android_sdk_roots(project_root) {
        let Ok(entries) = fs::read_dir(root.join("build-tools")) else {
            continue;
        };

        for entry in entries.flatten() {
            for name in apksigner_names() {
                candidates.push(entry.path().join(name));
            }
        }
    }

    let found = sort_newest_first(candidates)
        .into_iter()
        .find(|candidate| candidate.is_file());

    if let Some(path) = found.as_ref() {
        debug!(path = %path.display(), "Found apksigner in the Android SDK");
    }

    found
}

/// File names `apksigner` ships under, including the Windows launcher.
fn apksigner_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["apksigner.bat", "apksigner.exe"]
    } else {
        &[APKSIGNER]
    }
}

/// Android SDK locations, in the order Gradle/Flutter itself resolves them.
///
/// `ANDROID_HOME` and `ANDROID_SDK_ROOT` come first because they are what CI
/// images set; `<project>/android/local.properties` is what a developer machine
/// written by Android Studio usually has.
fn android_sdk_roots(project_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        let Some(value) = env::var_os(variable) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        roots.push(PathBuf::from(value));
    }

    if let Some(from_properties) =
        sdk_dir_from_local_properties(&project_root.join("android").join("local.properties"))
    {
        roots.push(from_properties);
    }

    roots.dedup();
    roots
}

/// Reads `sdk.dir` out of `android/local.properties`.
fn sdk_dir_from_local_properties(path: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(path).ok()?;

    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with('!'))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "sdk.dir").then(|| PathBuf::from(decode_property(value.trim())))
        })
}

/// Undoes the escaping Gradle uses when it writes `local.properties`.
///
/// Windows paths are written as `C\:\\Users\\me\\AppData\\Local\\Android\\Sdk`,
/// where `\\` is one backslash and `\:` is a literal colon.
fn decode_property(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }

        match characters.next() {
            Some(escaped) => decoded.push(escaped),
            None => decoded.push('\\'),
        }
    }

    decoded
}

/// Orders build-tools paths newest first, so the most recent version wins.
fn sort_newest_first(mut candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    candidates.sort_by(|left, right| {
        build_tools_version(right)
            .cmp(&build_tools_version(left))
            .then_with(|| left.cmp(right))
    });
    candidates
}

/// Numeric version of a `<sdk>/build-tools/<version>/apksigner` path.
fn build_tools_version(path: &Path) -> Vec<u32> {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components
        .iter()
        .position(|component| component == "build-tools")
        .and_then(|index| components.get(index + 1))
        // `34.0.0-rc3` and `34.0.0` describe the same version.
        .map(|version| {
            version
                .split(['-', '+'])
                .next()
                .unwrap_or(version)
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(0))
                .collect()
        })
        .unwrap_or_default()
}

/// `apksigner` password argument: by environment variable when possible.
fn password_spec(env_var: &str, password: &str) -> String {
    match env::var(env_var) {
        Ok(from_environment) if from_environment == password => format!("env:{env_var}"),
        _ => format!("pass:{password}"),
    }
}

/// First `Signer #N certificate DN:` subject in `apksigner verify --print-certs`.
fn parse_signer(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, subject) = line.split_once("certificate DN:")?;
        let subject = subject.trim();
        (!subject.is_empty()).then(|| subject.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_is_passed_by_name_when_it_comes_from_the_environment() {
        temp_env::with_var(
            AndroidConfig::KEYSTORE_PASSWORD_ENV,
            Some("hunter2"),
            || {
                assert_eq!(
                    password_spec(AndroidConfig::KEYSTORE_PASSWORD_ENV, "hunter2"),
                    "env:PACOR_ANDROID_KEYSTORE_PASSWORD"
                );
                // A different value came from the configuration file.
                assert_eq!(
                    password_spec(AndroidConfig::KEYSTORE_PASSWORD_ENV, "from-file"),
                    "pass:from-file"
                );
            },
        );
    }

    #[test]
    fn a_literal_password_falls_back_to_the_command_line() {
        temp_env::with_var(AndroidConfig::KEY_PASSWORD_ENV, None::<&str>, || {
            assert_eq!(
                password_spec(AndroidConfig::KEY_PASSWORD_ENV, "secret"),
                "pass:secret"
            );
        });
    }

    #[test]
    fn signer_subjects_are_read_from_apksigner_output() {
        let output = "\
Verifies
Verified using v1 scheme (JAR signing): true
Verified using v2 scheme (APK Signature Scheme v2): true
Number of signers: 1
Signer #1 certificate DN: CN=Android Debug, O=Android, C=US
Signer #1 certificate SHA-256 digest: 0123456789abcdef
";

        assert_eq!(
            parse_signer(output),
            Some("CN=Android Debug, O=Android, C=US".to_string())
        );
        assert!(parse_signer("Verifies\nNumber of signers: 1\n").is_none());
    }

    #[test]
    fn build_tools_are_ordered_newest_first() {
        let candidates = vec![
            PathBuf::from("/sdk/build-tools/33.0.2/apksigner"),
            PathBuf::from("/sdk/build-tools/35.0.0/apksigner"),
            PathBuf::from("/sdk/build-tools/35.0.0/apksigner.bat"),
            PathBuf::from("/sdk/build-tools/34.0.0-rc3/apksigner"),
        ];

        let ordered = sort_newest_first(candidates);

        assert_eq!(
            ordered[0],
            PathBuf::from("/sdk/build-tools/35.0.0/apksigner")
        );
        assert_eq!(
            ordered[1],
            PathBuf::from("/sdk/build-tools/35.0.0/apksigner.bat")
        );
        assert_eq!(
            ordered[2],
            PathBuf::from("/sdk/build-tools/34.0.0-rc3/apksigner"),
            "34.0.0-rc3 is newer than 33.0.2 and ties with 34.0.0"
        );
        assert_eq!(
            ordered.last().unwrap(),
            &PathBuf::from("/sdk/build-tools/33.0.2/apksigner")
        );
    }

    #[test]
    fn a_path_without_a_build_tools_version_sorts_last() {
        let candidates = vec![
            PathBuf::from("/opt/apksigner"),
            PathBuf::from("/sdk/build-tools/35.0.0/apksigner"),
        ];

        assert_eq!(
            sort_newest_first(candidates).last().unwrap(),
            &PathBuf::from("/opt/apksigner")
        );
        assert!(build_tools_version(Path::new("/opt/apksigner")).is_empty());
        assert_eq!(
            build_tools_version(Path::new("/sdk/build-tools/35.0.0/apksigner")),
            vec![35, 0, 0]
        );
    }

    #[test]
    fn gradle_escaped_sdk_paths_are_decoded() {
        assert_eq!(
            decode_property(r"C\:\\Users\\me\\AppData\\Local\\Android\\Sdk"),
            r"C:\Users\me\AppData\Local\Android\Sdk"
        );
        assert_eq!(
            decode_property("/usr/lib/android-sdk"),
            "/usr/lib/android-sdk"
        );
        assert_eq!(decode_property("C:/Android/sdk"), "C:/Android/sdk");
    }

    #[test]
    fn sdk_dir_is_read_from_local_properties() {
        let directory = tempfile::tempdir().unwrap();
        let properties = directory.path().join("local.properties");
        fs::write(
            &properties,
            "# comment\nsdk.dir=C\\:\\\\Android\\\\Sdk\nflutter.sdk=C\\:\\\\flutter\n",
        )
        .unwrap();

        assert_eq!(
            sdk_dir_from_local_properties(&properties),
            Some(PathBuf::from(r"C:\Android\Sdk")),
            "only sdk.dir is used"
        );

        fs::write(&properties, "flutter.sdk=C\\:\\\\flutter\n").unwrap();
        assert_eq!(sdk_dir_from_local_properties(&properties), None);
        assert_eq!(
            sdk_dir_from_local_properties(&directory.path().join("missing.properties")),
            None
        );
    }

    #[test]
    fn signing_requires_the_keystore_alias_and_both_passwords() {
        let directory = tempfile::tempdir().unwrap();
        let project_root = directory.path();

        temp_env::with_vars(
            [
                (AndroidConfig::KEYSTORE_PASSWORD_ENV, None::<&str>),
                (AndroidConfig::KEY_PASSWORD_ENV, None::<&str>),
            ],
            || {
                let mut config = AndroidConfig::default();
                assert!(ApkSigner::resolve(&config, project_root).is_err());

                config.keystore_file = Some("definitely-missing.jks".to_string());
                let error = ApkSigner::resolve(&config, project_root)
                    .err()
                    .expect("a missing keystore must be rejected")
                    .to_string();
                assert!(error.contains("does not exist"), "{error}");
            },
        );
    }

    #[test]
    fn the_configured_tool_path_must_exist() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = AndroidConfig::default();
        config.keystore_file = Some("upload.jks".to_string());
        config.signing_tool_path = Some(directory.path().join("apksigner"));

        let error = resolve_apksigner(&config, directory.path())
            .unwrap_err()
            .to_string();

        assert!(error.contains("does not exist"), "{error}");
    }

    #[test]
    fn the_sdk_is_searched_for_apksigner() {
        let directory = tempfile::tempdir().unwrap();
        let project_root = directory.path().join("app");
        let sdk = directory.path().join("sdk");
        let tool_name = apksigner_names()[0];

        // `34.0.0` exists but has no tool in it, so it must not win.
        for (version, present) in [("33.0.2", true), ("35.0.0", true), ("34.0.0", false)] {
            let tool = sdk.join("build-tools").join(version).join(tool_name);
            fs::create_dir_all(tool.parent().unwrap()).unwrap();
            if present {
                fs::write(&tool, b"#!/bin/sh\n").unwrap();
            }
        }

        fs::create_dir_all(project_root.join("android")).unwrap();
        fs::write(
            project_root.join("android").join("local.properties"),
            // Gradle escapes Windows paths, so `local.properties` holds
            // `C\:\\Android\\sdk` rather than the path itself.
            format!(
                "sdk.dir={}\n",
                sdk.display()
                    .to_string()
                    .replace('\\', "\\\\")
                    .replace(':', "\\:")
            ),
        )
        .unwrap();

        temp_env::with_vars(
            [
                ("ANDROID_HOME", None::<&str>),
                ("ANDROID_SDK_ROOT", None::<&str>),
            ],
            || {
                assert_eq!(
                    find_apksigner_in_android_sdk(&project_root),
                    Some(sdk.join("build-tools").join("35.0.0").join(tool_name)),
                    "the newest build-tools version wins"
                );
            },
        );
    }
}

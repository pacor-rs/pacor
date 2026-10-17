//! Android App Bundle signing with `jarsigner`.
//!
//! An `.aab` is not an APK: it is a JAR, and the APK signature schemes
//! `apksigner` implements do not apply to it. Google Play accepts a JAR
//! signature made with the upload key (it re-signs with the app signing key
//! under Play App Signing), which is what `jarsigner` produces.
//!
//! Signing is driven by the same `platforms.android` keystore as APK signing, so
//! a configured keystore signs both formats — the difference is only the tool.
//!
//! `flutter build appbundle` already signs the bundle with the project's Gradle
//! signing configuration (the **debug** keystore unless the project configures
//! one), and `jarsigner` *adds* a signature instead of replacing one. A bundle
//! carrying both certificates is not publishable, so the Gradle signature is
//! stripped first: the published bundle carries exactly one signer, the
//! configured upload key.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use pacor_core::{
    config::AndroidConfig,
    utils::command::{CommandOutputMode, execute_command, macros::command_args},
};

use super::{
    certificate::{Signer, first_line, is_debug_certificate},
    jdk,
};

/// How `jarsigner` is named on this host.
const JARSIGNER: &str = "jarsigner";

/// Digest and signature algorithms Play accepts, pinned so an old JDK default
/// (SHA-1) cannot silently produce a rejected bundle.
const SIG_ALG: &str = "SHA256withRSA";
const DIGEST_ALG: &str = "SHA-256";

/// Resolved signing configuration: a tool plus the keystore it uses.
pub(super) struct BundleSigner {
    tool: PathBuf,

    keystore: PathBuf,

    alias: String,

    /// Never logged or formatted by this type.
    keystore_password: String,

    /// Never logged or formatted by this type.
    key_password: String,
}

impl BundleSigner {
    /// Resolves the signing tool and validates the keystore configuration.
    ///
    /// # Errors
    /// Fails when the keystore is missing or absent from disk, the alias or
    /// either password is unset, or no `jarsigner` can be found.
    pub(super) fn resolve(config: &AndroidConfig, project_root: &Path) -> Result<Self> {
        let keystore = config.keystore_path(project_root).ok_or_else(|| {
            anyhow!("`platforms.android.keystore_file` must be set to sign App Bundles")
        })?;

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
            .ok_or_else(|| anyhow!("set `platforms.android.key_alias` to sign App Bundles"))?
            .to_string();

        let keystore_password = config.keystore_password().ok_or_else(|| {
            anyhow!(
                "set `PACOR_ANDROID_KEYSTORE_PASSWORD` (or `platforms.android.keystore_password`) \
                 to sign App Bundles"
            )
        })?;

        let key_password = config.key_password().ok_or_else(|| {
            anyhow!(
                "set `PACOR_ANDROID_KEY_PASSWORD` (or `platforms.android.key_password`) to sign App \
                 Bundles"
            )
        })?;

        let tool = resolve_jarsigner(config)?;

        debug!(
            tool = %tool.display(),
            keystore = %keystore.display(),
            alias = %alias,
            "Resolved the App Bundle signing toolchain"
        );

        Ok(Self {
            tool,
            keystore,
            alias,
            keystore_password,
            key_password,
        })
    }

    /// Signs one App Bundle in place.
    pub(super) fn sign(
        &self,
        bundle: &Path,
        command_output_mode: &CommandOutputMode,
    ) -> Result<()> {
        debug!(bundle = %bundle.display(), "Signing App Bundle");

        // Gradle signed this bundle already; leaving that signature in place
        // would publish a bundle carrying the debug certificate *as well as*
        // the configured upload key.
        replace_existing_signature(bundle)?;

        // `-storepass:env`/`-keypass:env` keep a password that came from the
        // environment out of the process list; a password read from the
        // configuration file has to be passed literally.
        let store_pass = password_args(
            "storepass",
            AndroidConfig::KEYSTORE_PASSWORD_ENV,
            &self.keystore_password,
        );
        let key_pass = password_args(
            "keypass",
            AndroidConfig::KEY_PASSWORD_ENV,
            &self.key_password,
        );

        let mut args = command_args!(
            capacity = 12,
            "-keystore",
            &self.keystore,
            store_pass.0.as_str(),
            store_pass.1.as_str(),
            key_pass.0.as_str(),
            key_pass.1.as_str(),
            "-sigalg",
            SIG_ALG,
            "-digestalg",
            DIGEST_ALG,
        );
        args.push(bundle.as_os_str().into());
        // `jarsigner [options] jar-file alias`
        args.push(self.alias.as_str().into());

        execute_command(
            &self.tool,
            &args,
            None,
            None::<&[(&str, &str)]>,
            command_output_mode,
        )
        .with_context(|| {
            format!(
                "failed to sign {}: re-run with `--command-output normal` to see jarsigner's own \
                 diagnostics",
                bundle.display()
            )
        })?;

        info!(bundle = %bundle.display(), "App Bundle signed");
        Ok(())
    }
}

/// Replaces the JAR signature Gradle produced with room for pacor's own.
///
/// `jarsigner` adds a signature to a signed JAR rather than replacing it, so
/// without this step the published bundle would carry the debug certificate
/// *and* the upload key (verified: signing `app-release.aab` produced both
/// `META-INF/ANDROIDD.*` and `META-INF/UPLOAD.*`). Google Play expects a single
/// upload-key signature, so every existing signature entry is dropped and
/// `jarsigner` rebuilds the manifest for the new signer.
fn replace_existing_signature(bundle: &Path) -> Result<()> {
    let stripped = bundle.with_extension("unsigned");

    match rewrite_without_signatures(bundle, &stripped) {
        Ok(removed) => {
            // Windows cannot rename over an existing file.
            fs::remove_file(bundle)
                .with_context(|| format!("failed to replace {}", bundle.display()))?;
            fs::rename(&stripped, bundle)
                .with_context(|| format!("failed to replace {}", bundle.display()))?;
            debug!(
                entries = removed,
                "Removed the existing JAR signature from the App Bundle"
            );
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&stripped);
            Err(error)
        }
    }
}

/// Copies every non-signature entry of `source` into `target`.
///
/// Returns how many signature entries were dropped.
fn rewrite_without_signatures(source: &Path, target: &Path) -> Result<usize> {
    let file =
        fs::File::open(source).with_context(|| format!("failed to read {}", source.display()))?;
    let mut archive = ZipArchive::new(file).with_context(|| {
        format!(
            "{} is not a readable Android App Bundle (expected a zip)",
            source.display()
        )
    })?;

    let mut writer = ZipWriter::new(
        fs::File::create(target)
            .with_context(|| format!("failed to write {}", target.display()))?,
    );
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::DEFLATE);

    let mut removed = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();

        if is_signature_entry(&name) {
            removed += 1;
            continue;
        }

        if name.ends_with('/') {
            writer.add_directory(name, options)?;
        } else {
            writer.start_file(name, options)?;
            io::copy(&mut entry, &mut writer)?;
        }
    }

    writer.finish()?;
    Ok(removed)
}

/// Whether a zip entry belongs to a JAR signature.
///
/// Only the signature files themselves are matched: `META-INF/MANIFEST.MF` and
/// `META-INF/*.SF|RSA|DSA|EC|SIG` at the top level. Anything else below
/// `META-INF/` (services, version files, …) is payload and has to survive.
fn is_signature_entry(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    if upper == "META-INF/MANIFEST.MF" {
        return true;
    }

    let Some(rest) = upper.strip_prefix("META-INF/") else {
        return false;
    };

    if rest.contains('/') {
        return false;
    }

    matches!(
        rest.rsplit_once('.').map(|(_, extension)| extension),
        Some("SF" | "RSA" | "DSA" | "EC" | "SIG")
    )
}

/// Inspects the certificate of an already built bundle, best effort.
///
/// Runs only when pacor did not sign the bundle itself, where the signature is
/// whatever Gradle produced — Flutter's template signs release builds with the
/// **debug** keystore, and Google Play rejects such a bundle.
pub(super) fn inspect_bundle(config: &AndroidConfig, bundle: &Path) -> Signer {
    let Some(tool) = find_jarsigner(config) else {
        debug!("jarsigner was not found; skipping the App Bundle signer check");
        return Signer::Unknown;
    };

    // `-verify` needs neither the keystore nor its passwords.
    let args = command_args!("-verify", "-verbose", "-certs", bundle);
    let output = match Command::new(&tool).args(&args).output() {
        Ok(output) => output,
        Err(error) => {
            debug!(tool = %tool.display(), %error, "Failed to run jarsigner -verify");
            return Signer::Unknown;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(subject) = parse_signer(&stdout).or_else(|| parse_signer(&stderr)) {
        return Signer::from_subject(subject);
    }

    // Jarsigner reports both of these in the output rather than by exit code.
    let combined = format!("{stdout}\n{stderr}");
    if let Some(reason) = refusal_reason(&combined) {
        return Signer::Unverified(reason);
    }

    if output.status.success() {
        return Signer::Unknown;
    }

    Signer::Unverified(
        first_line(&stderr)
            .or_else(|| first_line(&stdout))
            .unwrap_or_else(|| "jarsigner -verify failed".to_string()),
    )
}

/// Finds `jarsigner`: explicit configuration, then `PATH`, then a JDK.
fn resolve_jarsigner(config: &AndroidConfig) -> Result<PathBuf> {
    if let Some(configured) = config.jarsigner_path.as_ref() {
        if !configured.is_file() {
            bail!(
                "the configured signing tool {} does not exist",
                configured.display()
            );
        }
        return Ok(configured.clone());
    }

    if let Some(found) = find_jarsigner(config) {
        return Ok(found);
    }

    bail!(
        "no App Bundle signing tool found: install a JDK (or Android Studio, which bundles one), \
         put `jarsigner` on `PATH`, or set `platforms.android.jarsigner_path`"
    )
}

/// Same lookup as [`resolve_jarsigner`], but `None` instead of an error.
fn find_jarsigner(config: &AndroidConfig) -> Option<PathBuf> {
    if let Some(configured) = config.jarsigner_path.as_ref().filter(|tool| tool.is_file()) {
        return Some(configured.clone());
    }

    jdk::find_tool(JARSIGNER)
}

/// `-storepass`/`-keypass` argument pair, by environment variable when possible.
fn password_args(flag: &str, env_var: &str, password: &str) -> (String, String) {
    match env::var(env_var) {
        Ok(from_environment) if from_environment == password => {
            (format!("-{flag}:env"), env_var.to_string())
        }
        _ => (format!("-{flag}"), password.to_string()),
    }
}

/// Subject of the first `X.509, <subject>` line in `jarsigner -verify -certs`.
fn parse_signer(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, subject) = line.split_once("X.509,")?;
        let subject = subject.trim();
        (!subject.is_empty()).then(|| subject.to_string())
    })
}

/// Why `jarsigner` considers the bundle unusable, when it says so in its output.
///
/// `-verify` reports an unsigned or tampered jar in prose, so the exit code
/// alone would be reported as "unknown" instead of "unsigned".
fn refusal_reason(output: &str) -> Option<String> {
    let lowercased = output.to_lowercase();

    for (needle, reason) in [
        ("jar is unsigned", "the App Bundle is not signed"),
        (
            "not signed by alias",
            "the App Bundle is not signed by the expected key",
        ),
        (
            "has been tampered",
            "the App Bundle signature does not verify (tampered)",
        ),
    ] {
        if lowercased.contains(needle) {
            return Some(reason.to_string());
        }
    }

    if is_debug_certificate(output) {
        return Some("signed with the Android debug certificate".to_string());
    }

    None
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
                    password_args("storepass", AndroidConfig::KEYSTORE_PASSWORD_ENV, "hunter2"),
                    (
                        "-storepass:env".to_string(),
                        "PACOR_ANDROID_KEYSTORE_PASSWORD".to_string()
                    )
                );
                // A different value came from the configuration file.
                assert_eq!(
                    password_args(
                        "storepass",
                        AndroidConfig::KEYSTORE_PASSWORD_ENV,
                        "from-file"
                    ),
                    ("-storepass".to_string(), "from-file".to_string())
                );
            },
        );
    }

    #[test]
    fn a_literal_password_falls_back_to_the_command_line() {
        temp_env::with_var(AndroidConfig::KEY_PASSWORD_ENV, None::<&str>, || {
            assert_eq!(
                password_args("keypass", AndroidConfig::KEY_PASSWORD_ENV, "secret"),
                ("-keypass".to_string(), "secret".to_string())
            );
        });
    }

    #[test]
    fn signer_subjects_are_read_from_jarsigner_output() {
        let output = "\
jar verified.

Warning: \n
         42 Mon Jan 01 00:00:00 UTC 2024 META-INF/MANIFEST.MF
            >>> Signer
            X.509, CN=Android Debug, O=Android, C=US
            [certificate is valid from 1/1/24 to 1/1/44]
";

        assert_eq!(
            parse_signer(output),
            Some("CN=Android Debug, O=Android, C=US".to_string())
        );
        assert!(parse_signer("jar verified.\n").is_none());
    }

    #[test]
    fn refusal_reasons_are_recognised() {
        assert_eq!(
            refusal_reason("jar is unsigned. (signatures missing or not parsable)"),
            Some("the App Bundle is not signed".to_string())
        );
        assert_eq!(
            refusal_reason("Warning: This jar contains entries whose signer certificate ..."),
            None
        );
        assert!(refusal_reason("verified by CN=My Upload Key").is_none());
        assert_eq!(
            refusal_reason("X.509, CN=Android Debug, O=Android, C=US"),
            Some("signed with the Android debug certificate".to_string())
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
                assert!(BundleSigner::resolve(&config, project_root).is_err());

                config.keystore_file = Some("definitely-missing.jks".to_string());
                let error = BundleSigner::resolve(&config, project_root)
                    .err()
                    .expect("a missing keystore must be rejected")
                    .to_string();
                assert!(error.contains("does not exist"), "{error}");
                assert!(error.contains("keystore"), "{error}");
            },
        );
    }

    #[test]
    fn the_configured_tool_path_must_exist() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = AndroidConfig::default();
        config.jarsigner_path = Some(directory.path().join("jarsigner"));

        let error = resolve_jarsigner(&config).unwrap_err().to_string();

        assert!(error.contains("does not exist"), "{error}");
    }

    /// A JDK that is not on `PATH` still has to be found (the lookup itself is
    /// covered by `jdk`'s tests; this pins it for the bundle signer).
    #[test]
    fn a_jdk_home_is_searched_for_jarsigner() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("jdk");
        let tool = home.join("bin").join(jdk::executable_name("jarsigner"));
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(&tool, b"binary").unwrap();

        // `PATH` is emptied so a jarsigner installed on the developer's machine
        // cannot be found before the JDK home under test.
        temp_env::with_var("JAVA_HOME", Some(home.clone()), || {
            temp_env::with_var("PATH", Some(directory.path().as_os_str()), || {
                assert_eq!(
                    find_jarsigner(&AndroidConfig::default()),
                    Some(tool.clone())
                );
            });
        });
    }

    #[test]
    fn signature_entries_are_recognised_and_payload_is_kept() {
        for signature in [
            "META-INF/MANIFEST.MF",
            "META-INF/CERT.SF",
            "META-INF/CERT.RSA",
            "META-INF/ANDROIDD.DSA",
            "meta-inf/upload.ec",
            "META-INF/SIG.SIG",
        ] {
            assert!(is_signature_entry(signature), "{signature}");
        }

        for payload in [
            "AndroidManifest.xml",
            "base/dex/classes.dex",
            "META-INF/services/com.example.Service",
            "META-INF/1.0.0/composeResources/x.xml",
            "META-INF/MANIFEST", // not the manifest itself
            "META-INF/NOTICE.txt",
        ] {
            assert!(!is_signature_entry(payload), "{payload}");
        }
    }

    /// The published bundle must carry one signer, not Gradle's *and* pacor's.
    #[test]
    fn rewriting_drops_the_signature_and_keeps_the_bundle_contents() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("app-release.aab");
        write_bundle(
            &source,
            &[
                ("AndroidManifest.xml", b"<manifest/>"),
                ("base/dex/classes.dex", b"dex"),
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0"),
                ("META-INF/CERT.SF", b"signature"),
                ("META-INF/CERT.RSA", b"certificate"),
                ("META-INF/services/com.example.Service", b"service"),
            ],
        );

        let target = directory.path().join("stripped.aab");
        let removed = rewrite_without_signatures(&source, &target).unwrap();

        assert_eq!(removed, 3, "manifest plus the two signature files");
        assert_eq!(
            bundle_entries(&target),
            vec![
                "AndroidManifest.xml",
                "META-INF/services/com.example.Service",
                "base/dex/classes.dex",
            ],
            "payload survives, signature and manifest are gone"
        );
        assert_eq!(bundle_entry(&target, "base/dex/classes.dex"), b"dex");
    }

    #[test]
    fn replacing_the_signature_leaves_no_temporary_file_behind() {
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("app-release.aab");
        write_bundle(
            &bundle,
            &[
                ("AndroidManifest.xml", b"<manifest/>"),
                ("META-INF/CERT.RSA", b"certificate"),
            ],
        );

        replace_existing_signature(&bundle).unwrap();

        assert_eq!(bundle_entries(&bundle), vec!["AndroidManifest.xml"]);
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            1,
            "the intermediate file is renamed over the bundle"
        );
    }

    #[test]
    fn a_bundle_that_is_not_a_zip_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("app-release.aab");
        fs::write(&bundle, b"not a zip").unwrap();

        let error = replace_existing_signature(&bundle).unwrap_err().to_string();

        assert!(
            error.contains("not a readable Android App Bundle"),
            "{error}"
        );
        assert!(bundle.exists(), "the input is left untouched");
    }

    fn write_bundle(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
        let options = SimpleFileOptions::default();
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            io::Write::write_all(&mut writer, contents).unwrap();
        }
        writer.finish().unwrap();
    }

    fn bundle_entries(path: &Path) -> Vec<String> {
        let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect();
        names.sort();
        names
    }

    fn bundle_entry(path: &Path, name: &str) -> Vec<u8> {
        let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut entry = archive.by_name(name).unwrap();
        let mut contents = Vec::new();
        io::Read::read_to_end(&mut entry, &mut contents).unwrap();
        contents
    }
}

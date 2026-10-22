//! End-to-end wiring check for the Android backend.
//!
//! Everything runs against a hand-written fixture project plus stub `flutter`
//! and `jarsigner` executables that only record their arguments, so no Android
//! SDK, JDK or real Flutter installation is involved — yet the parts unit tests
//! cannot reach are covered: project discovery, environment validation, the
//! `flutter build apk`/`appbundle` argument vectors, artifact collection and
//! naming, signing order, checksum coverage and cleanup.
//!
//! The tests in this binary mutate the process working directory and `PATH`,
//! which are process-global, so they serialise themselves through [`FIXTURE`].

use std::{
    env,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pacor_core::{
    config::PacorConfig,
    core::builder::BundleBuilder,
    utils::{command::CommandOutputMode, crypto::calculate_sha1},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use pacor_packaging_android::AndroidBuilder;

/// Serialises the tests that change `PATH` and the working directory.
static FIXTURE: Mutex<()> = Mutex::new(());

/// Runs `test` while holding the fixture lock, releasing it even on panic.
fn serialised(test: impl FnOnce()) {
    let _guard = FIXTURE.lock().unwrap_or_else(|poison| poison.into_inner());
    test();
}

/// A fixture Flutter project plus stub tools in front of `PATH`.
struct Fixture {
    directory: tempfile::TempDir,
    previous_directory: PathBuf,
    previous_path: Option<OsString>,
}

impl Fixture {
    fn new(android_section: &str) -> Self {
        let directory = tempfile::tempdir().expect("failed to create the fixture");
        let root = directory.path();

        fs::create_dir_all(root.join("android")).unwrap();
        fs::write(
            root.join("pubspec.yaml"),
            "name: probe_app\nversion: 1.0.0+1\ndescription: probe\n",
        )
        .unwrap();
        fs::write(root.join("pacor.toml"), config_file(android_section)).unwrap();

        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        write_stub_flutter(&bin, &root.join("flutter-stub.log"));
        write_stub_jarsigner(
            &bin,
            &root.join("jarsigner-stub.log"),
            &root.join("jarsigner-verify.txt"),
        );
        write_stub_apksigner(
            &bin,
            &root.join("apksigner-stub.log"),
            &root.join("apksigner-verify.txt"),
        );

        let previous_directory = env::current_dir().unwrap();
        let previous_path = env::var_os("PATH");
        // `set_var` is unsafe in edition 2024: the caller guarantees that no
        // other thread reads the environment concurrently, which the fixture
        // lock provides.
        unsafe { env::set_var("PATH", prepend_to_path(&bin)) };
        env::set_current_dir(root).unwrap();

        Self {
            directory,
            previous_directory,
            previous_path,
        }
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    /// Loads the fixture configuration. The builder borrows it, so it has to
    /// outlive the builder in every test.
    fn config(&self) -> PacorConfig {
        PacorConfig::load_from(Some(&self.root().join("pacor.toml")))
            .expect("the fixture configuration must load")
    }

    /// Writes a stand-in for one of Flutter's split APKs.
    fn stage_apk(&self, name: &str, size: usize) {
        let directory = self.root().join("build/app/outputs/flutter-apk");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(name), vec![b'x'; size]).unwrap();
    }

    /// Writes a stand-in for Flutter's App Bundle.
    fn stage_bundle(&self, relative: &str, size: usize) {
        let path = self.root().join("build/app/outputs/bundle").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; size]).unwrap();
    }

    /// Writes a real zip, which is what an App Bundle is.
    ///
    /// Signing strips the signature Gradle put there, so that path needs a
    /// readable archive rather than arbitrary bytes.
    fn stage_bundle_zip(&self, relative: &str, entries: &[(&str, &[u8])]) {
        let path = self.root().join("build/app/outputs/bundle").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();

        let mut writer = ZipWriter::new(fs::File::create(&path).unwrap());
        let options = SimpleFileOptions::default();
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    /// Output the stub `jarsigner -verify` prints.
    fn set_verify_output(&self, output: &str) {
        fs::write(self.root().join("jarsigner-verify.txt"), output).unwrap();
    }

    /// Output the stub `apksigner verify` prints.
    fn set_apk_verify_output(&self, output: &str) {
        fs::write(self.root().join("apksigner-verify.txt"), output).unwrap();
    }

    fn flutter_calls(&self) -> Vec<String> {
        read_lines(&self.root().join("flutter-stub.log"))
    }

    fn jarsigner_calls(&self) -> Vec<String> {
        read_lines(&self.root().join("jarsigner-stub.log"))
    }

    fn apksigner_calls(&self) -> Vec<String> {
        read_lines(&self.root().join("apksigner-stub.log"))
    }

    fn dist_files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.root().join("dist"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Asserts that `file`'s `.sha1` describes the file as published.
    fn assert_checksum_matches(&self, file: &str) {
        let path = self.root().join("dist").join(file);
        let recorded = fs::read_to_string(path.with_extension(format!(
            "{}.sha1",
            path.extension().unwrap().to_string_lossy()
        )))
        .unwrap()
        .split("  ")
        .next()
        .unwrap()
        .to_string();

        assert_eq!(
            recorded,
            calculate_sha1(&path).unwrap(),
            "the checksum must cover the published bytes of {file}"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.previous_directory);
        match self.previous_path.take() {
            // SAFETY: see `Fixture::new`; the fixture lock is still held.
            Some(path) => unsafe { env::set_var("PATH", path) },
            None => unsafe { env::remove_var("PATH") },
        }
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn config_file(android_section: &str) -> String {
    format!(
        r#"[app]
default_locale = "en-US"
url = "https://example.com"

[app.i18n.en-US]
name = "Probe"
publisher = "Probe Inc"

[build]
output_dir = "dist"

[platforms.android]
{android_section}
"#
    )
}

/// Records the arguments it is called with and succeeds without doing anything.
fn write_stub_flutter(bin: &Path, log: &Path) {
    let log = log.display();

    // Windows resolves `flutter` to `flutter.bat`; everywhere else the bare file
    // is used and has to be executable.
    fs::write(
        bin.join("flutter.bat"),
        format!("@echo off\r\necho %* >> \"{log}\"\r\nexit /b 0\r\n"),
    )
    .unwrap();
    fs::write(
        bin.join("flutter"),
        format!("#!/bin/sh\necho \"$@\" >> \"{log}\"\nexit 0\n"),
    )
    .unwrap();

    make_executable(&bin.join("flutter"));
}

/// Records its arguments, signs by appending a marker to the bundle, and prints
/// the configured `-verify` output.
///
/// The marker makes it observable whether pacor generated the checksum before or
/// after signing; the verify output is a file so a test can choose between a
/// debug and a release certificate.
fn write_stub_jarsigner(bin: &Path, log: &Path, verify_output: &Path) {
    let log = log.display();
    let verify_output = verify_output.display();

    fs::write(
        bin.join("jarsigner.bat"),
        format!(
            "@echo off\r\n\
             echo %* >> \"{log}\"\r\n\
             if \"%1\"==\"-verify\" goto verify\r\n\
             for %%a in (%*) do if /I \"%%~xa\"==\".aab\" set BUNDLE=%%a\r\n\
             echo jar-stub-signature>> \"%BUNDLE%\"\r\n\
             echo jar signed.\r\n\
             exit /b 0\r\n\
             :verify\r\n\
             type \"{verify_output}\"\r\n\
             exit /b 0\r\n"
        ),
    )
    .unwrap();

    fs::write(
        bin.join("jarsigner"),
        format!(
            "#!/bin/sh\n\
             echo \"$@\" >> \"{log}\"\n\
             if [ \"$1\" = \"-verify\" ]; then cat \"{verify_output}\"; exit 0; fi\n\
             for argument in \"$@\"; do case \"$argument\" in *.aab) bundle=\"$argument\";; esac; done\n\
             echo jar-stub-signature >> \"$bundle\"\n\
             echo 'jar signed.'\n\
             exit 0\n"
        ),
    )
    .unwrap();

    make_executable(&bin.join("jarsigner"));
}

/// Records its arguments, signs by appending a marker to the APK, and prints the
/// configured `verify` output.
///
/// The marker makes it observable whether pacor generated the checksum before or
/// after signing. The stub never writes a `.idsig`; pacor must not ask for one,
/// which the call log shows.
fn write_stub_apksigner(bin: &Path, log: &Path, verify_output: &Path) {
    let log = log.display();
    let verify_output = verify_output.display();

    fs::write(
        bin.join("apksigner.bat"),
        format!(
            "@echo off\r\n\
             echo %* >> \"{log}\"\r\n\
             if \"%1\"==\"verify\" goto verify\r\n\
             for %%a in (%*) do if /I \"%%~xa\"==\".apk\" set APK=%%a\r\n\
             echo apk-stub-signature>> \"%APK%\"\r\n\
             echo signed\r\n\
             exit /b 0\r\n\
             :verify\r\n\
             type \"{verify_output}\"\r\n\
             exit /b 0\r\n"
        ),
    )
    .unwrap();

    fs::write(
        bin.join("apksigner"),
        format!(
            "#!/bin/sh\n\
             echo \"$@\" >> \"{log}\"\n\
             if [ \"$1\" = \"verify\" ]; then cat \"{verify_output}\"; exit 0; fi\n\
             for argument in \"$@\"; do case \"$argument\" in *.apk) apk=\"$argument\";; esac; done\n\
             echo apk-stub-signature >> \"$apk\"\n\
             echo 'signed'\n\
             exit 0\n"
        ),
    )
    .unwrap();

    make_executable(&bin.join("apksigner"));
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn prepend_to_path(bin: &Path) -> OsString {
    let mut paths = vec![bin.to_path_buf()];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    env::join_paths(paths).unwrap()
}

/// Environment variables that would make the fixture machine-dependent.
///
/// `JAVA_HOME` and the Android SDK variables would let the signer checks find a
/// real `jarsigner`/`apksigner` instead of the stubs.
const ISOLATED_ENVIRONMENT: [(&str, Option<&str>); 7] = [
    ("PACOR_OUTPUT_DIR", None),
    ("PACOR_BUILD_MODE", None),
    ("ANDROID_HOME", None),
    ("ANDROID_SDK_ROOT", None),
    ("JAVA_HOME", None),
    ("PACOR_ANDROID_KEYSTORE_PASSWORD", None),
    ("PACOR_ANDROID_KEY_PASSWORD", None),
];

#[test]
fn the_android_backend_packages_apks_without_adb_or_an_android_sdk() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new("target_abi = [\"arm64-v8a\"]");
            fixture.stage_apk("app-arm64-v8a-release.apk", 128);

            // `validate` must pass with `flutter` alone: adb is not a packaging
            // tool, and requiring it made this fail on machines whose
            // `platform-tools` is simply not on `PATH`.
            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must not require adb");

            // Packaging an existing build must not invoke Flutter at all.
            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("packaging the staged APK must succeed");
            assert_eq!(artifacts.len(), 1);
            assert!(
                fixture.flutter_calls().is_empty(),
                "--use-existing-flutter-build must skip `flutter build`"
            );

            assert_eq!(
                fixture.dist_files(),
                vec![
                    "probe_app-1.0.0+1-arm64-v8a.apk".to_string(),
                    "probe_app-1.0.0+1-arm64-v8a.apk.sha1".to_string(),
                ]
            );
            fixture.assert_checksum_matches("probe_app-1.0.0+1-arm64-v8a.apk");

            // A real build passes the ABI and the symbol directory to Flutter.
            let artifacts = builder
                .build(false, CommandOutputMode::None)
                .expect("packaging after the (stubbed) Flutter build must succeed");
            assert_eq!(artifacts.len(), 1);
            let calls = fixture.flutter_calls();
            assert_eq!(calls.len(), 1, "one `flutter build` invocation: {calls:?}");
            for expected in [
                "build apk --release",
                "--split-per-abi",
                "--split-debug-info=tmp/",
                "--target-platform android-arm64",
            ] {
                assert!(
                    calls[0].contains(expected),
                    "`{expected}` missing from `{}`",
                    calls[0]
                );
            }

            // Split debug symbols live in the project and survive until cleanup,
            // which is what makes `--skip-cleanup` meaningful.
            let symbols = fixture.root().join("tmp");
            fs::create_dir_all(&symbols).unwrap();
            fs::write(symbols.join("app.android-arm64.symbols"), b"symbols").unwrap();
            builder.clean().expect("cleanup must succeed");
            assert!(!symbols.exists(), "cleanup removes the symbol directory");
        });
    });
}

#[test]
fn a_configured_keystore_signs_the_apk_with_apksigner() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nkeystore_file = \"android/upload.jks\"\nkey_alias = \"upload\"\nkey_password = \"key-from-file\"\nsigning_tool_path = \"APKSIGNER_PATH\"",
            );
            fs::write(
                fixture.root().join("android/upload.jks"),
                b"not a real keystore",
            )
            .unwrap();
            fixture.stage_apk("app-arm64-v8a-release.apk", 256);
            point_at_stub(&fixture, "signing_tool_path");

            unsafe { env::set_var("PACOR_ANDROID_KEYSTORE_PASSWORD", "store-from-env") };

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("signing the APK must succeed");
            assert_eq!(artifacts.len(), 1);

            let calls = fixture.apksigner_calls();
            assert_eq!(calls.len(), 1, "one signing invocation: {calls:?}");
            let call = &calls[0];
            for expected in [
                "sign",
                "--ks",
                "upload.jks",
                "--ks-key-alias",
                "upload",
                "--ks-pass",
                "env:PACOR_ANDROID_KEYSTORE_PASSWORD",
                "--key-pass",
                "pass:key-from-file",
                // v4 would make apksigner drop an `.idsig` in dist that pacor
                // neither reports nor checksums.
                "--v4-signing-enabled false",
            ] {
                assert!(
                    call.contains(expected),
                    "`{expected}` missing from `{call}`"
                );
            }

            // The stub appended to the APK, so a checksum computed before signing
            // would not match the published file.
            fixture.assert_checksum_matches("probe_app-1.0.0+1-arm64-v8a.apk");
            assert_eq!(
                fixture.dist_files(),
                vec![
                    "probe_app-1.0.0+1-arm64-v8a.apk".to_string(),
                    "probe_app-1.0.0+1-arm64-v8a.apk.sha1".to_string(),
                ],
                "dist must contain exactly the reported artifacts"
            );
            let published =
                fs::read(fixture.root().join("dist/probe_app-1.0.0+1-arm64-v8a.apk")).unwrap();
            assert!(published.len() > 256, "the signed APK is the published one");
        });
    });
}

/// The real signer DN from `apksigner verify --print-certs` is
/// `C=US, O=Android, CN=Android Debug` — not the `CN=Android Debug, O=…` an
/// earlier version of this test assumed, so the check has to be a substring one.
#[test]
fn a_debug_signed_apk_is_reported_and_left_untouched() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nsigning_tool_path = \"APKSIGNER_PATH\"",
            );
            fixture.stage_apk("app-arm64-v8a-release.apk", 256);
            fixture.set_apk_verify_output(
                "Verifies\nSigner #1 certificate DN: C=US, O=Android, CN=Android Debug\n",
            );
            point_at_stub(&fixture, "signing_tool_path");

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("a debug-signed APK must still be packaged");
            assert_eq!(artifacts.len(), 1);

            let calls = fixture.apksigner_calls();
            assert_eq!(calls.len(), 1, "one `apksigner verify`: {calls:?}");
            assert!(calls[0].starts_with("verify --print-certs"), "{}", calls[0]);

            // Nothing was signed by pacor, so the APK must be published as-is.
            assert_eq!(
                fs::read(fixture.root().join("dist/probe_app-1.0.0+1-arm64-v8a.apk"))
                    .unwrap()
                    .len(),
                256
            );
        });
    });
}

#[test]
fn an_app_bundle_is_opt_in_and_packaged_alongside_the_apk() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            // Only `jarsigner_path` points at the stub; no keystore is
            // configured, so signing stays off and only the inspection runs.
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nformats = [\"apk\", \"aab\"]\njarsigner_path = \"JARSIGNER_PATH\"",
            );
            fixture.stage_apk("app-arm64-v8a-release.apk", 128);
            fixture.stage_bundle("release/app-release.aab", 256);
            fixture
                .set_verify_output("jar verified.\n    X.509, CN=My Upload Key, O=Example, C=CN\n");
            point_at_stub(&fixture, "jarsigner_path");

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            // Both artifacts are packaged from an existing build.
            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("packaging the staged artifacts must succeed");
            assert_eq!(artifacts.len(), 2, "one APK and one bundle");
            assert_eq!(
                fixture.dist_files(),
                vec![
                    "probe_app-1.0.0+1-arm64-v8a.apk".to_string(),
                    "probe_app-1.0.0+1-arm64-v8a.apk.sha1".to_string(),
                    "probe_app-1.0.0+1.aab".to_string(),
                    "probe_app-1.0.0+1.aab.sha1".to_string(),
                ]
            );
            fixture.assert_checksum_matches("probe_app-1.0.0+1.aab");
            fixture.assert_checksum_matches("probe_app-1.0.0+1-arm64-v8a.apk");

            // The signer check ran against the bundle, using the tool path. `contains` rather
            // than `ends_with`: the Windows stub records the command line with `%*`, which
            // keeps the quotes cmd put around the path, so the line ends with `"`.
            let calls = fixture.jarsigner_calls();
            assert_eq!(calls.len(), 1, "one `jarsigner -verify`: {calls:?}");
            assert!(calls[0].starts_with("-verify"), "{}", calls[0]);
            assert!(calls[0].contains(".aab"), "{}", calls[0]);

            // A real build runs Flutter twice: apk (split) then appbundle.
            builder
                .build(false, CommandOutputMode::None)
                .expect("packaging after the (stubbed) Flutter build must succeed");

            let calls = fixture.flutter_calls();
            assert_eq!(calls.len(), 2, "two `flutter build` invocations: {calls:?}");
            assert!(calls[0].starts_with("build apk --release"), "{}", calls[0]);
            assert!(
                calls[0].contains("--split-per-abi"),
                "APKs are split per ABI: {}",
                calls[0]
            );
            assert!(
                calls[1].starts_with("build appbundle --release"),
                "{}",
                calls[1]
            );
            assert!(
                !calls[1].contains("--split-per-abi"),
                "a bundle is never split: {}",
                calls[1]
            );
            for call in &calls {
                assert!(call.contains("--split-debug-info=tmp/"), "{call}");
                assert!(call.contains("--target-platform android-arm64"), "{call}");
            }
        });
    });
}

#[test]
fn a_bundle_only_configuration_builds_no_apk() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nformats = [\"aab\"]\njarsigner_path = \"JARSIGNER_PATH\"",
            );
            fixture.stage_apk("app-arm64-v8a-release.apk", 128);
            fixture.stage_bundle("release/app-release.aab", 256);
            fixture.set_verify_output("jar verified.\n");
            point_at_stub(&fixture, "jarsigner_path");

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("packaging the staged bundle must succeed");

            assert_eq!(artifacts.len(), 1, "only the bundle");
            assert_eq!(
                fixture.dist_files(),
                vec![
                    "probe_app-1.0.0+1.aab".to_string(),
                    "probe_app-1.0.0+1.aab.sha1".to_string(),
                ],
                "the staged APK must not be packaged"
            );

            builder
                .build(false, CommandOutputMode::None)
                .expect("packaging after the (stubbed) Flutter build must succeed");
            let calls = fixture.flutter_calls();
            assert_eq!(calls.len(), 1, "only `flutter build appbundle`: {calls:?}");
            assert!(
                calls[0].starts_with("build appbundle"),
                "no APK is built: {}",
                calls[0]
            );
        });
    });
}

#[test]
fn a_configured_keystore_signs_the_bundle_with_jarsigner() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nformats = [\"aab\"]\nkeystore_file = \"android/upload-keystore.jks\"\nkey_alias = \"upload\"\nkey_password = \"key-from-file\"\njarsigner_path = \"JARSIGNER_PATH\"",
            );
            fs::write(
                fixture.root().join("android/upload-keystore.jks"),
                b"not a real keystore",
            )
            .unwrap();
            // A real zip, already carrying the signature Gradle would have made.
            fixture.stage_bundle_zip(
                "release/app-release.aab",
                &[
                    (
                        "AndroidManifest.xml",
                        b"<manifest package=\"com.example\"/>",
                    ),
                    ("base/dex/classes.dex", b"dex"),
                    ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0"),
                    ("META-INF/ANDROIDD.SF", b"gradle signature"),
                    ("META-INF/ANDROIDD.RSA", b"gradle certificate"),
                ],
            );
            point_at_stub(&fixture, "jarsigner_path");

            unsafe { env::set_var("PACOR_ANDROID_KEYSTORE_PASSWORD", "store-from-env") };

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("signing the bundle must succeed");
            assert_eq!(artifacts.len(), 1);

            let calls = fixture.jarsigner_calls();
            assert_eq!(calls.len(), 1, "one signing invocation: {calls:?}");
            let call = &calls[0];
            for expected in [
                "-keystore",
                "upload-keystore.jks",
                "-storepass:env",
                "PACOR_ANDROID_KEYSTORE_PASSWORD",
                "-keypass",
                "key-from-file",
                "-digestalg",
                "SHA-256",
            ] {
                assert!(
                    call.contains(expected),
                    "`{expected}` missing from `{call}`"
                );
            }
            // `-sigalg` must stay absent: `jarsigner` derives it from the key, and pinning
            // SHA256withRSA rejects every EC keystore.
            assert!(
                !call.contains("-sigalg"),
                "sigalg must not be pinned: {call}"
            );
            // `jarsigner` takes the alias as the last positional argument. Strip a closing
            // quote first: the Windows stub records the line through `%*`, which keeps the
            // quotes cmd put around the path.
            let trimmed = call.trim_end().trim_end_matches('"');
            assert!(trimmed.ends_with("upload"), "the alias is last: {call}");

            // Gradle's signature is gone, so the published bundle carries one
            // signer (the configured upload key) instead of two.
            let published = fixture.root().join("dist/probe_app-1.0.0+1.aab");
            let entries = bundle_entries(&published);
            assert!(
                !entries.iter().any(|name| name.contains("ANDROIDD")),
                "Gradle's signature entry survived: {entries:?}"
            );
            assert!(
                !entries.contains(&"META-INF/MANIFEST.MF".to_string()),
                "the old manifest is rebuilt by jarsigner: {entries:?}"
            );
            assert_eq!(
                bundle_entry(&published, "base/dex/classes.dex"),
                b"dex",
                "payload is preserved"
            );

            // The stub appended to the bundle, so a checksum computed before
            // signing would not match the published file.
            fixture.assert_checksum_matches("probe_app-1.0.0+1.aab");
            let published_bytes = fs::read(&published).unwrap();
            assert!(
                published_bytes.len() > 256,
                "the published bundle is the signed one (the stub appended a marker), got {} bytes",
                published_bytes.len()
            );
        });
    });
}

/// The zip entries of a published artifact, for asserting what signing did.
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
    std::io::Read::read_to_end(&mut entry, &mut contents).unwrap();
    contents
}

#[test]
fn a_debug_signed_bundle_is_reported() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nformats = [\"aab\"]\njarsigner_path = \"JARSIGNER_PATH\"",
            );
            fixture.stage_bundle_zip(
                "release/app-release.aab",
                &[
                    (
                        "AndroidManifest.xml",
                        b"<manifest package=\"com.example\"/>",
                    ),
                    ("META-INF/ANDROIDD.SF", b"gradle signature"),
                    ("META-INF/ANDROIDD.RSA", b"gradle certificate"),
                ],
            );
            fixture
                .set_verify_output("jar verified.\n    X.509, CN=Android Debug, O=Android, C=US\n");
            point_at_stub(&fixture, "jarsigner_path");

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            // The warning itself goes to the log; what matters here is that the
            // inspection happened and did not fail the build (Play rejects such
            // a bundle, but the artifact is still produced for local testing).
            let artifacts = builder
                .build(true, CommandOutputMode::None)
                .expect("a debug-signed bundle must still be packaged");
            assert_eq!(artifacts.len(), 1);
            assert_eq!(fixture.jarsigner_calls().len(), 1);

            // Nothing was signed by pacor, so Gradle's signature has to survive:
            // stripping it here would publish an *unsigned* bundle, which is
            // worse than the debug-signed one it replaced.
            let entries = bundle_entries(&fixture.root().join("dist/probe_app-1.0.0+1.aab"));
            assert!(
                entries.iter().any(|name| name.contains("ANDROIDD")),
                "the existing signature must be left alone: {entries:?}"
            );
        });
    });
}

#[test]
fn a_missing_bundle_names_the_expected_file_and_the_flavor() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nformats = [\"aab\"]\nflavor = \"prod\"\njarsigner_path = \"JARSIGNER_PATH\"",
            );
            fixture.stage_bundle("release/app-release.aab", 256);
            point_at_stub(&fixture, "jarsigner_path");

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            let Err(error) = builder.build(true, CommandOutputMode::None) else {
                panic!("a missing App Bundle must fail the build");
            };
            let error = error.to_string();

            assert!(
                error.contains("did not produce an Android App Bundle"),
                "{error}"
            );
            assert!(error.contains("app-prod-release.aab"), "{error}");
            assert!(error.contains("app-release.aab"), "{error}");
        });
    });
}

#[test]
fn an_empty_format_list_is_rejected_before_anything_is_built() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new("target_abi = [\"arm64-v8a\"]\nformats = []");
            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");

            let error = builder
                .validate()
                .expect_err("an empty format list must be rejected")
                .to_string();

            assert!(error.contains("platforms.android.formats"), "{error}");
            assert!(error.contains("at least one format"), "{error}");
            assert!(
                fixture.dist_files().is_empty(),
                "validation must run before any artifact is written"
            );
        });
    });
}

#[test]
fn an_empty_abi_list_is_rejected_before_anything_is_built() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            // An empty ABI list used to silently fall back to arm64-v8a while
            // Flutter built every ABI, so fewer APKs were published than the
            // configuration promised.
            let fixture = Fixture::new("target_abi = []");
            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");

            let error = builder
                .validate()
                .expect_err("an empty ABI list must be rejected")
                .to_string();

            assert!(error.contains("platforms.android.target_abi"), "{error}");
            assert!(error.contains("at least one ABI"), "{error}");
        });
    });
}

#[test]
fn an_incomplete_signing_configuration_is_rejected_up_front() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            // A configured keystore that is not there has to fail early: signing
            // used to be skipped after an otherwise successful build.
            let fixture = Fixture::new(
                "target_abi = [\"arm64-v8a\"]\nkeystore_file = \"android/missing.jks\"\nkey_alias = \"upload\"",
            );
            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");

            let error = builder
                .validate()
                .expect_err("a missing keystore must be rejected")
                .to_string();

            assert!(error.contains("missing.jks"), "{error}");
            assert!(error.contains("does not exist"), "{error}");
        });

        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            // ... and a present keystore without passwords must not be silently
            // downgraded to "unsigned artifact" either.
            let fixture = Fixture::new("target_abi = [\"arm64-v8a\"]");
            fs::write(
                fixture.root().join("android/upload-keystore.jks"),
                b"not a real keystore",
            )
            .unwrap();
            fs::write(
                fixture.root().join("pacor.toml"),
                config_file(
                    "target_abi = [\"arm64-v8a\"]\nkeystore_file = \"android/upload-keystore.jks\"\nkey_alias = \"upload\"",
                ),
            )
            .unwrap();

            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            let error = builder
                .validate()
                .expect_err("signing without passwords must be rejected")
                .to_string();

            assert!(error.contains("PACOR_ANDROID_KEYSTORE_PASSWORD"), "{error}");
        });
    });
}

#[test]
fn an_abi_flutter_did_not_build_is_reported_with_the_directory_contents() {
    serialised(|| {
        temp_env::with_vars(ISOLATED_ENVIRONMENT, || {
            let fixture = Fixture::new("target_abi = [\"arm64-v8a\", \"armeabi-v7a\"]");
            fixture.stage_apk("app-arm64-v8a-release.apk", 128);

            // Same order as the CLI: validation first, so the output directory
            // exists by the time the build writes into it.
            let config = fixture.config();
            let builder =
                AndroidBuilder::new(&config).expect("the fixture project must be readable");
            builder.validate().expect("validation must pass");

            // `BuildArtifact` intentionally implements no `Debug`, so the error
            // is destructured rather than read through `expect_err`.
            let Err(error) = builder.build(true, CommandOutputMode::None) else {
                panic!("a missing ABI APK must fail the build");
            };
            let error = error.to_string();

            assert!(
                error.contains("did not produce an APK for armeabi-v7a"),
                "{error}"
            );
            assert!(error.contains("app-armeabi-v7a-release.apk"), "{error}");
            assert!(
                error.contains("app-arm64-v8a-release.apk"),
                "the listing of what the directory does contain is part of the error: {error}"
            );
        });
    });
}

/// Replaces the `JARSIGNER_PATH`/`APKSIGNER_PATH` placeholders with the stubs.
///
/// The stubs are only reachable through the configured tool paths; relying on
/// `PATH` discovery would let a developer machine's real JDK or Android SDK sign
/// the fixture.
fn point_at_stub(fixture: &Fixture, key: &str) {
    let stub = |name: &str| {
        let file = if cfg!(windows) {
            format!("{name}.bat")
        } else {
            name.to_string()
        };
        // TOML strings treat a backslash as an escape.
        fixture
            .root()
            .join("bin")
            .join(file)
            .display()
            .to_string()
            .replace('\\', "/")
    };

    let path = fixture.root().join("pacor.toml");
    let contents = fs::read_to_string(&path)
        .unwrap()
        .replace("JARSIGNER_PATH", &stub("jarsigner"))
        .replace("APKSIGNER_PATH", &stub("apksigner"));

    assert!(
        !contents.contains("JARSIGNER_PATH") && !contents.contains("APKSIGNER_PATH"),
        "every tool placeholder must be resolved"
    );

    fs::write(&path, contents).unwrap();

    assert!(
        fs::read_to_string(&path).unwrap().contains(key),
        "the fixture configuration must still configure `{key}`"
    );
}

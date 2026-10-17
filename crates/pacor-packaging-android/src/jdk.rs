//! Locating the JDK the Android signing tools need.
//!
//! `jarsigner` *is* a JDK tool, and `apksigner.bat` is a launcher that runs
//! `java` — without `JAVA_HOME` or a `java` on `PATH` it fails with
//! `ERROR: JAVA_HOME is not set`. Flutter resolves Android Studio's bundled JBR
//! for its own Gradle runs, but a shell that only runs `pacor` usually has
//! neither, so pacor resolves a JDK itself and hands it to those tools.

use std::{env, fs, path::PathBuf};

use pacor_core::utils::command::find_command_path;

/// Executable file name for `tool` on this host.
pub(super) fn executable_name(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

/// Candidate JDK installation directories, most specific first.
///
/// `JAVA_HOME` is what CI images and Gradle set; Android Studio's bundled JBR is
/// the JDK Flutter itself uses for Android builds, so a machine that can run
/// `flutter build apk` almost always has a JDK there.
pub(super) fn homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();

    if let Some(java_home) = env::var_os("JAVA_HOME").filter(|value| !value.is_empty()) {
        homes.push(PathBuf::from(java_home));
    }

    if cfg!(windows) {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            let Some(root) = env::var_os(variable) else {
                continue;
            };
            let root = PathBuf::from(root);
            homes.push(root.join("Android/Android Studio/jbr"));
            homes.push(root.join("Programs/Android Studio/jbr"));
        }
    } else {
        homes.push(PathBuf::from(
            "/Applications/Android Studio.app/Contents/jbr/Contents/Home",
        ));
        homes.push(PathBuf::from("/opt/android-studio/jbr"));

        // Linux distributions keep every JDK side by side here.
        if let Ok(entries) = fs::read_dir("/usr/lib/jvm") {
            homes.extend(entries.flatten().map(|entry| entry.path()));
        }
    }

    homes.dedup();
    homes
}

/// A JDK home that actually has a `java` binary, for tools that need `JAVA_HOME`.
pub(super) fn home_for_tools() -> Option<PathBuf> {
    homes()
        .into_iter()
        .find(|home| home.join("bin").join(executable_name("java")).is_file())
}

/// Finds `tool` on `PATH` and then in the JDK candidates.
pub(super) fn find_tool(tool: &str) -> Option<PathBuf> {
    if let Ok(Some(found)) = find_command_path(tool) {
        return Some(found);
    }

    let name = executable_name(tool);
    let found = homes()
        .into_iter()
        .map(|home| home.join("bin").join(&name))
        .find(|candidate| candidate.is_file());

    if let Some(path) = found.as_ref() {
        debug!(path = %path.display(), tool, "Found a JDK tool");
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_names_carry_the_windows_extension() {
        if cfg!(windows) {
            assert_eq!(executable_name("jarsigner"), "jarsigner.exe");
        } else {
            assert_eq!(executable_name("jarsigner"), "jarsigner");
        }
    }

    /// A JDK that is not on `PATH` still has to be found, and only a home that
    /// really contains `java` may be handed to `apksigner` as `JAVA_HOME`.
    #[test]
    fn a_jdk_home_is_searched_for_tools() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("jdk");
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(executable_name("jarsigner")), b"binary").unwrap();
        fs::write(bin.join(executable_name("java")), b"binary").unwrap();

        let empty = directory.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        // `PATH` is emptied so a JDK installed on the developer's machine cannot
        // be found before the home under test.
        temp_env::with_var("JAVA_HOME", Some(home.clone()), || {
            temp_env::with_var("PATH", Some(empty.as_os_str()), || {
                assert_eq!(
                    find_tool("jarsigner"),
                    Some(bin.join(executable_name("jarsigner")))
                );
                assert_eq!(home_for_tools(), Some(home.clone()));
            });
        });
    }

    /// A `JAVA_HOME` without `java` in it must not be handed to `apksigner`.
    #[test]
    fn a_home_without_java_is_not_used_for_tools() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("not-a-jdk");
        fs::create_dir_all(home.join("bin")).unwrap();

        temp_env::with_var("JAVA_HOME", Some(home), || {
            assert_ne!(
                home_for_tools(),
                Some(directory.path().join("not-a-jdk")),
                "a directory without bin/java is not a JDK home"
            );
        });
    }
}

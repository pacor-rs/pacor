use std::path::Path;

use anyhow::Result;

/// Create a .tar.gz archive from a source directory.
pub fn create_gzip_archive(
    source_dir: &Path,
    destination_path: &Path,
) -> Result<std::path::PathBuf> {
    use std::{fs, io::Write};

    use flate2::{Compression, write::GzEncoder};

    let gz_file = fs::File::create(destination_path)?;

    let gz_encoder = GzEncoder::new(gz_file, Compression::default());

    let mut tar_builder = tar::Builder::new(gz_encoder);
    tar_builder.follow_symlinks(false);
    let root_name = source_dir
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("source path has no file name: {}", source_dir.display()))?;
    tar_builder.append_dir_all(root_name, source_dir)?;

    let gz_encoder = tar_builder.into_inner()?;

    let mut gz_file = gz_encoder.finish()?;
    gz_file.flush()?;

    Ok(destination_path.to_owned())
}

/// Extract a ZIP archive to the given path.
///
/// If the archive contains a single root directory, it will be unwrapped,
/// so the contents are extracted directly into `path`.
pub fn extract_zip(data: &[u8], path: &Path) -> Result<()> {
    use zip::{ZipArchive, read::root_dir_common_filter};

    let cursor = std::io::Cursor::new(data);

    let mut archive = ZipArchive::new(cursor)?;
    if let Err(e) = archive.extract_unwrapped_root_dir(path, root_dir_common_filter) {
        error!(error = ?e, "Failed to extract ZIP archive");
        return Err(e.into());
    }

    Ok(())
}

pub fn create_zip_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    source_dir: P,
    zip_file_path: Q,
) -> Result<()> {
    use walkdir::WalkDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    let source_dir = source_dir.as_ref();
    let zip_file_path = zip_file_path.as_ref();

    let file = std::fs::File::create(zip_file_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::DEFLATE);

    for entry in WalkDir::new(source_dir) {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path.strip_prefix(source_dir)?;

        // Skip the archive root itself.
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        // ZIP requires forward slashes regardless of host.
        let zip_path = relative_path
            .components()
            .map(|c| c.as_os_str())
            .collect::<Vec<_>>()
            .join("/".as_ref());

        if path.is_file() {
            zip.start_file(zip_path.to_string_lossy(), options)?;
            let mut file = std::fs::File::open(path)?;
            std::io::copy(&mut file, &mut zip)?;
        } else if path.is_dir() {
            let dir_path = format!("{}/", zip_path.to_string_lossy());
            zip.add_directory(&dir_path, options)?;
        }
        // Symlinks are skipped: WalkDir does not follow links by default.
    }

    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    use tempfile::TempDir;

    /// Build `<dir>/<name>/...` with the given relative files.
    fn source_tree(dir: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = dir.join(name);
        for (relative, contents) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        root
    }

    #[test]
    fn gzip_archive_round_trips_file_contents() {
        let temp = TempDir::new().unwrap();
        let source = source_tree(
            temp.path(),
            "MyApp.app",
            &[
                ("Contents/Info.plist", "plist"),
                ("Contents/MacOS/MyApp", "binary"),
            ],
        );
        let archive = temp.path().join("MyApp.app.tar.gz");

        create_gzip_archive(&source, &archive).unwrap();

        // Decompress and read the entries back: this is what an updater does.
        let file = fs::File::open(&archive).unwrap();
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);

        let mut found = Vec::new();
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.header().entry_type().is_file() {
                let path = entry.path().unwrap().to_string_lossy().into_owned();
                let mut contents = String::new();
                std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
                found.push((path, contents));
            }
        }

        assert!(
            found
                .iter()
                .any(|(path, contents)| path.ends_with("Info.plist") && contents == "plist"),
            "expected Info.plist in archive, got {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(path, contents)| path.ends_with("MacOS/MyApp") && contents == "binary"),
            "expected the executable in archive, got {found:?}"
        );
    }

    #[test]
    fn gzip_archive_preserves_the_root_directory_name() {
        let temp = TempDir::new().unwrap();
        let source = source_tree(temp.path(), "MyApp.app", &[("marker.txt", "x")]);
        let archive = temp.path().join("out.tar.gz");

        create_gzip_archive(&source, &archive).unwrap();

        let file = fs::File::open(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let first = tar.entries().unwrap().next().unwrap().unwrap();
        let path = first.path().unwrap().to_string_lossy().into_owned();

        // The updater extracts with the bundle name intact.
        assert!(
            path.starts_with("MyApp.app"),
            "unexpected root entry: {path}"
        );
    }

    #[test]
    fn gzip_archive_rejects_a_source_without_a_file_name() {
        let temp = TempDir::new().unwrap();
        let error =
            create_gzip_archive(Path::new("/"), &temp.path().join("out.tar.gz")).unwrap_err();
        assert!(error.to_string().contains("no file name"));
    }

    #[test]
    fn zip_archive_round_trips_file_contents() {
        let temp = TempDir::new().unwrap();
        let source = source_tree(
            temp.path(),
            "bundle",
            &[("app.exe", "binary"), ("data/assets.json", "{}")],
        );
        let archive = temp.path().join("portable.zip");

        create_zip_archive(&source, &archive).unwrap();

        // Read back through the same code path used for tool downloads.
        let data = fs::read(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).unwrap();

        let mut names = Vec::new();
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).unwrap();
            let mut contents = String::new();
            std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
            names.push((entry.name().to_string(), contents));
        }

        assert!(
            names
                .iter()
                .any(|(name, contents)| name == "app.exe" && contents == "binary"),
            "expected app.exe at the archive root, got {names:?}"
        );
        assert!(
            names
                .iter()
                .any(|(name, contents)| name == "data/assets.json" && contents == "{}"),
            "expected nested asset with forward slashes, got {names:?}"
        );
    }

    #[test]
    fn extract_zip_unwraps_a_single_root_directory() {
        let temp = TempDir::new().unwrap();
        let source = source_tree(temp.path(), "inner", &[("file.txt", "content")]);
        let archive = temp.path().join("wrapped.zip");
        create_zip_archive(&source, &archive).unwrap();

        let target = temp.path().join("extracted");
        fs::create_dir_all(&target).unwrap();

        extract_zip(&fs::read(&archive).unwrap(), &target).unwrap();

        // The `inner/` wrapper is removed, so the file lands at the root.
        assert_eq!(
            fs::read_to_string(target.join("file.txt")).unwrap(),
            "content"
        );
    }
}

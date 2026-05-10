use std::path::Path;

use anyhow::Result;

/// Create a .tar.gz archive from a source directory.
#[cfg(target_os = "macos")]
pub(crate) fn create_gzip_archive(
    source_dir: &Path,
    destination_path: &Path,
) -> Result<std::path::PathBuf> {
    use std::{fs, io::Write};

    use flate2::{Compression, write::GzEncoder};

    let gz_file = fs::File::create(destination_path)?;

    let gz_encoder = GzEncoder::new(gz_file, Compression::default());

    let mut tar_builder = tar::Builder::new(gz_encoder);
    tar_builder.follow_symlinks(false);
    tar_builder.append_dir_all(
        source_dir
            .file_name()
            .expect("Source path has no file name"),
        source_dir,
    )?;

    let gz_encoder = tar_builder.into_inner()?;

    let mut gz_file = gz_encoder.finish()?;
    gz_file.flush()?;

    Ok(destination_path.to_owned())
}

/// Extract a ZIP archive to the given path.
///
/// If the archive contains a single root directory, it will be unwrapped,
/// so the contents are extracted directly into `path`.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn extract_zip(data: &[u8], path: &Path) -> Result<()> {
    use zip::{ZipArchive, read::root_dir_common_filter};

    let cursor = std::io::Cursor::new(data);

    let mut archive = ZipArchive::new(cursor)?;
    if let Err(e) = archive.extract_unwrapped_root_dir(path, root_dir_common_filter) {
        error!(error = ?e, "Failed to extract ZIP archive");
        return Err(e.into());
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn create_zip_archive<P: AsRef<Path>, Q: AsRef<Path>>(
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

        // 跳过根目录
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        // 转换为 ZIP 标准路径（使用 / 分隔符）
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
        // 注意：符号链接被忽略，因为 WalkDir 默认 follow_links(false)
    }

    zip.finish()?;
    Ok(())
}

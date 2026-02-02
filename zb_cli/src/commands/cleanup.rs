use console::style;
use std::path::Path;
use zb_core::Error;
use zb_io::install::Installer;

/// Clean unreferenced store entries, the download cache, and temporary files under `root`.
///
/// This performs a three-stage cleanup:
/// 1. Runs garbage collection via the provided `installer` and reports unreferenced store entries.
/// 2. Scans `root/cache` and removes either incomplete downloads (files ending with `.part`) or all entries when `scrub` is true.
/// 3. Scans and removes all entries under `root/tmp`.
///
/// - `dry_run`: when true, the command only reports what would be removed and does not delete anything.
/// - `scrub`: when true, remove all entries in the download cache instead of only `.part` files.
///
/// Errors are returned if `installer.gc()` fails or if required directories cannot be read.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// // assuming `installer` is available from the application context
/// let mut installer = /* obtain Installer */ unimplemented!();
/// let root = Path::new("/path/to/project");
/// // Dry run to preview removals
/// let _ = zb_cli::commands::cleanup::execute(&mut installer, root, true, false);
/// ```
pub fn execute(
    installer: &mut Installer,
    root: &Path,
    dry_run: bool,
    scrub: bool,
) -> Result<(), Error> {
    println!(
        "{} {}",
        style("==>").cyan().bold(),
        if dry_run {
            "Would clean up the following..."
        } else {
            "Cleaning up..."
        }
    );

    let mut total_bytes: u64 = 0;
    let mut cleaned_items = 0;

    // 1. Garbage collect unreferenced store entries
    let unreferenced = installer.gc()?;
    if !unreferenced.is_empty() {
        println!("\n{} Unreferenced store entries:", style("==>").cyan());
        for key in &unreferenced {
            let store_path = root.join("store").join(key);
            if store_path.exists() {
                total_bytes += get_dir_size(&store_path);
                println!("    {} {}", style("•").dim(), &key[..12]);
                cleaned_items += 1;
            }
        }
    }

    // 2. Clean download cache
    let cache_path = root.join("cache");
    if cache_path.exists() {
        let cache_entries: Vec<_> = std::fs::read_dir(&cache_path)
            .map_err(|e| Error::FileError {
                message: format!("failed to read cache directory: {e}"),
            })?
            .filter_map(|e| e.ok())
            .collect();

        if !cache_entries.is_empty() {
            println!("\n{} Download cache:", style("==>").cyan());

            for entry in cache_entries {
                let path = entry.path();
                let size = if path.is_file() {
                    std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
                } else {
                    get_dir_size(&path)
                };

                // In scrub mode, remove everything
                // Otherwise, only remove .part files (incomplete downloads)
                let should_remove = scrub
                    || path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.ends_with(".part"))
                        .unwrap_or(false);

                if should_remove {
                    total_bytes += size;
                    println!(
                        "    {} {} ({})",
                        style("•").dim(),
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        format_bytes(size)
                    );

                    if !dry_run {
                        if path.is_file() {
                            let _ = std::fs::remove_file(&path);
                        } else {
                            let _ = std::fs::remove_dir_all(&path);
                        }
                    }
                    cleaned_items += 1;
                }
            }
        }
    }

    // 3. Clean tmp directory
    let tmp_path = root.join("tmp");
    if tmp_path.exists() {
        let tmp_entries: Vec<_> = std::fs::read_dir(&tmp_path)
            .map_err(|e| Error::FileError {
                message: format!("failed to read tmp directory: {e}"),
            })?
            .filter_map(|e| e.ok())
            .collect();

        if !tmp_entries.is_empty() {
            println!("\n{} Temporary files:", style("==>").cyan());

            for entry in tmp_entries {
                let path = entry.path();
                let size = if path.is_file() {
                    std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
                } else {
                    get_dir_size(&path)
                };

                total_bytes += size;
                println!(
                    "    {} {} ({})",
                    style("•").dim(),
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    format_bytes(size)
                );

                if !dry_run {
                    if path.is_file() {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        let _ = std::fs::remove_dir_all(&path);
                    }
                }
                cleaned_items += 1;
            }
        }
    }

    // Summary
    println!();
    if cleaned_items == 0 {
        println!("Already clean! Nothing to clean up.");
    } else if dry_run {
        println!(
            "{} Would free {} from {} item(s)",
            style("==>").cyan().bold(),
            style(format_bytes(total_bytes)).green().bold(),
            cleaned_items
        );
        println!(
            "{}",
            style("Run without --dry-run to actually clean up").dim()
        );
    } else {
        println!(
            "{} Freed {} from {} item(s)",
            style("==>").cyan().bold(),
            style(format_bytes(total_bytes)).green().bold(),
            cleaned_items
        );
    }

    Ok(())
}

/// Compute the total size, in bytes, of all files contained within `path` and its subdirectories.
///
/// This ignores directory entries that cannot be read and files whose metadata cannot be retrieved; such entries do not contribute to the returned size.
///
/// # Examples
///
/// ```
/// use std::fs::{create_dir_all, File};
/// use std::io::Write;
/// use tempfile::tempdir;
///
/// let dir = tempdir().unwrap();
/// let sub = dir.path().join("sub");
/// create_dir_all(&sub).unwrap();
/// let mut f1 = File::create(dir.path().join("a.txt")).unwrap();
/// f1.write_all(&vec![0u8; 10]).unwrap();
/// let mut f2 = File::create(sub.join("b.txt")).unwrap();
/// f2.write_all(&vec![0u8; 20]).unwrap();
///
/// let total = get_dir_size(dir.path());
/// assert_eq!(total, 30);
/// ```
fn get_dir_size(path: &Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                size += std::fs::metadata(&entry_path).map(|m| m.len()).unwrap_or(0);
            } else if entry_path.is_dir() {
                size += get_dir_size(&entry_path);
            }
        }
    }
    size
}

/// Format a byte count into a human-readable string using 1024-based units (B, KB, MB, GB).
///
/// Values at KB and above are shown with two decimal places.
///
/// # Examples
///
/// ```
/// assert_eq!(format_bytes(500), "500 B");
/// assert_eq!(format_bytes(2048), "2.00 KB");
/// assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
/// ```
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
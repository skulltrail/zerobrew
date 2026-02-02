use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::api::ApiClient;
use crate::db::Database;
use crate::download::{DownloadRequest, ParallelDownloader};
use crate::progress::{InstallProgress, ProgressCallback};

use zb_core::{Cask, Error};

/// Cask installer handles downloading and installing macOS casks
pub struct CaskInstaller {
    api_client: ApiClient,
    downloader: ParallelDownloader,
    db: Database,
    applications_dir: PathBuf,
    caskroom_dir: PathBuf,
}

pub struct CaskInstallResult {
    pub installed: usize,
}

impl CaskInstaller {
    pub fn new(
        api_client: ApiClient,
        downloader: ParallelDownloader,
        db: Database,
        applications_dir: PathBuf,
        caskroom_dir: PathBuf,
    ) -> Self {
        Self {
            api_client,
            downloader,
            db,
            applications_dir,
            caskroom_dir,
        }
    }

    /// Fetch cask metadata from API
    pub async fn get_cask(&self, name: &str) -> Result<Cask, Error> {
        self.api_client.get_cask(name).await
    }

    /// Install a cask
    pub async fn install(
        &mut self,
        name: &str,
        progress: Option<Arc<ProgressCallback>>,
    ) -> Result<CaskInstallResult, Error> {
        let report = |event: InstallProgress| {
            if let Some(ref cb) = progress {
                cb(event);
            }
        };

        // Fetch cask metadata
        let cask = self.api_client.get_cask(name).await?;

        // Check if already installed
        if let Some(existing) = self.db.get_installed_cask(&cask.token) {
            if existing.version == cask.version {
                return Ok(CaskInstallResult { installed: 0 });
            }
        }

        // Create caskroom directory for this cask
        let cask_dir = self.caskroom_dir.join(&cask.token).join(&cask.version);
        std::fs::create_dir_all(&cask_dir).map_err(|e| Error::FileError {
            message: format!("failed to create caskroom directory: {e}"),
        })?;

        // Download the cask artifact
        let sha256 = cask.sha256.clone().unwrap_or_else(|| "no_check".to_string());
        let request = DownloadRequest {
            url: cask.url.clone(),
            sha256: sha256.clone(),
            name: cask.token.clone(),
        };

        report(InstallProgress::DownloadStarted {
            name: cask.token.clone(),
            total_bytes: None,
        });

        let download_progress = progress.clone().map(|cb| {
            Arc::new(move |event: InstallProgress| {
                cb(event);
            }) as Arc<dyn Fn(InstallProgress) + Send + Sync>
        });

        // Download
        let blob_path = if sha256 == "no_check" {
            // For casks without checksum, download without verification
            self.downloader
                .download_single_no_verify(request, download_progress)
                .await?
                .blob_path
        } else {
            self.downloader
                .download_single(request, download_progress)
                .await?
        };

        report(InstallProgress::DownloadCompleted {
            name: cask.token.clone(),
            total_bytes: 0,
        });

        report(InstallProgress::UnpackStarted {
            name: cask.token.clone(),
        });

        // Extract/process the downloaded artifact
        let app_path = self.extract_cask(&cask, &blob_path, &cask_dir).await?;

        report(InstallProgress::UnpackCompleted {
            name: cask.token.clone(),
        });

        // Link app to Applications directory if it's a .app bundle
        let installed_app_path = if let Some(ref app) = app_path {
            report(InstallProgress::LinkStarted {
                name: cask.token.clone(),
            });

            let app_name = Path::new(app).file_name().unwrap().to_string_lossy();
            let dest = self.applications_dir.join(app_name.as_ref());

            // Create symlink to the app in caskroom
            let source = cask_dir.join(app);

            if dest.exists() {
                // Remove existing symlink or warn
                if dest.is_symlink() {
                    std::fs::remove_file(&dest).map_err(|e| Error::FileError {
                        message: format!("failed to remove existing symlink: {e}"),
                    })?;
                }
            }

            #[cfg(unix)]
            std::os::unix::fs::symlink(&source, &dest).map_err(|e| Error::FileError {
                message: format!("failed to create app symlink: {e}"),
            })?;

            report(InstallProgress::LinkCompleted {
                name: cask.token.clone(),
            });

            Some(dest.to_string_lossy().to_string())
        } else {
            None
        };

        // Record in database
        let tx = self.db.transaction()?;
        tx.record_cask_install(&cask.token, &cask.version, installed_app_path.as_deref())?;

        if let Some(ref app) = app_path {
            tx.record_cask_artifact(&cask.token, "app", app, &cask_dir.to_string_lossy())?;
        }

        tx.commit()?;

        report(InstallProgress::InstallCompleted {
            name: cask.token.clone(),
        });

        Ok(CaskInstallResult { installed: 1 })
    }

    /// Extract cask artifact based on file type
    async fn extract_cask(
        &self,
        cask: &Cask,
        blob_path: &Path,
        dest_dir: &Path,
    ) -> Result<Option<String>, Error> {
        let url = &cask.url;
        let url_lower = url.to_lowercase();

        if url_lower.ends_with(".dmg") {
            self.extract_dmg(cask, blob_path, dest_dir).await
        } else if url_lower.ends_with(".zip") {
            self.extract_zip(blob_path, dest_dir).await?;
            // Find .app in extracted contents
            Ok(self.find_app_bundle(dest_dir)?)
        } else if url_lower.ends_with(".pkg") {
            // PKG files need special handling (installer packages)
            // For now, just copy to caskroom
            let pkg_name = format!("{}.pkg", cask.token);
            let dest_pkg = dest_dir.join(&pkg_name);
            std::fs::copy(blob_path, &dest_pkg).map_err(|e| Error::FileError {
                message: format!("failed to copy pkg: {e}"),
            })?;
            Ok(Some(pkg_name))
        } else if url_lower.ends_with(".tar.gz") || url_lower.ends_with(".tgz") {
            self.extract_tarball(blob_path, dest_dir)?;
            Ok(self.find_app_bundle(dest_dir)?)
        } else if url_lower.ends_with(".app.zip") || url_lower.contains(".app") {
            self.extract_zip(blob_path, dest_dir).await?;
            Ok(self.find_app_bundle(dest_dir)?)
        } else {
            // Unknown format - just copy the file
            let file_name = url
                .split('/')
                .last()
                .unwrap_or(&cask.token)
                .split('?')
                .next()
                .unwrap_or(&cask.token);
            let dest_file = dest_dir.join(file_name);
            std::fs::copy(blob_path, &dest_file).map_err(|e| Error::FileError {
                message: format!("failed to copy artifact: {e}"),
            })?;
            Ok(None)
        }
    }

    /// Extract DMG file (macOS only)
    #[cfg(target_os = "macos")]
    async fn extract_dmg(
        &self,
        cask: &Cask,
        dmg_path: &Path,
        dest_dir: &Path,
    ) -> Result<Option<String>, Error> {
        use std::process::Command;

        // Create a temp mount point
        let mount_point = dest_dir.join(".mount");
        std::fs::create_dir_all(&mount_point).map_err(|e| Error::FileError {
            message: format!("failed to create mount point: {e}"),
        })?;

        // Mount the DMG
        let output = Command::new("hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-mountpoint",
                &mount_point.to_string_lossy(),
                &dmg_path.to_string_lossy(),
            ])
            .output()
            .map_err(|e| Error::CaskError {
                message: format!("failed to mount DMG: {e}"),
            })?;

        if !output.status.success() {
            return Err(Error::CaskError {
                message: format!(
                    "failed to mount DMG: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        // Find and copy .app bundle
        let app_path = self.find_app_bundle(&mount_point)?;
        let result = if let Some(ref app) = app_path {
            let source = mount_point.join(app);
            let dest = dest_dir.join(app);

            // Copy the app bundle
            self.copy_dir_all(&source, &dest)?;

            Some(app.clone())
        } else {
            None
        };

        // Unmount the DMG
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_point.to_string_lossy()])
            .output();

        // Clean up mount point
        let _ = std::fs::remove_dir(&mount_point);

        Ok(result)
    }

    /// Extract DMG file (non-macOS - returns error)
    #[cfg(not(target_os = "macos"))]
    async fn extract_dmg(
        &self,
        _cask: &Cask,
        _dmg_path: &Path,
        _dest_dir: &Path,
    ) -> Result<Option<String>, Error> {
        Err(Error::CaskError {
            message: "DMG extraction is only supported on macOS".to_string(),
        })
    }

    /// Extract ZIP file
    async fn extract_zip(&self, zip_path: &Path, dest_dir: &Path) -> Result<(), Error> {
        let file = std::fs::File::open(zip_path).map_err(|e| Error::FileError {
            message: format!("failed to open zip: {e}"),
        })?;

        let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::CaskError {
            message: format!("failed to read zip: {e}"),
        })?;

        archive.extract(dest_dir).map_err(|e| Error::CaskError {
            message: format!("failed to extract zip: {e}"),
        })?;

        Ok(())
    }

    /// Extract tarball
    fn extract_tarball(&self, tarball_path: &Path, dest_dir: &Path) -> Result<(), Error> {
        let file = std::fs::File::open(tarball_path).map_err(|e| Error::FileError {
            message: format!("failed to open tarball: {e}"),
        })?;

        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);

        archive.unpack(dest_dir).map_err(|e| Error::CaskError {
            message: format!("failed to extract tarball: {e}"),
        })?;

        Ok(())
    }

    /// Find .app bundle in a directory
    fn find_app_bundle(&self, dir: &Path) -> Result<Option<String>, Error> {
        let entries = std::fs::read_dir(dir).map_err(|e| Error::FileError {
            message: format!("failed to read directory: {e}"),
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| Error::FileError {
                message: format!("failed to read entry: {e}"),
            })?;

            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".app") {
                return Ok(Some(name));
            }
        }

        Ok(None)
    }

    /// Copy directory recursively
    #[cfg(target_os = "macos")]
    fn copy_dir_all(&self, src: &Path, dst: &Path) -> Result<(), Error> {
        use std::process::Command;

        // Use ditto for reliable macOS app bundle copying
        let output = Command::new("ditto")
            .args([&src.to_string_lossy().to_string(), &dst.to_string_lossy().to_string()])
            .output()
            .map_err(|e| Error::FileError {
                message: format!("failed to copy with ditto: {e}"),
            })?;

        if !output.status.success() {
            return Err(Error::FileError {
                message: format!(
                    "ditto failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        Ok(())
    }

    /// Uninstall a cask
    pub fn uninstall(&mut self, token: &str) -> Result<(), Error> {
        let installed = self
            .db
            .get_installed_cask(token)
            .ok_or(Error::CaskNotInstalled {
                name: token.to_string(),
            })?;

        // Get artifacts to remove (for future use)
        let _artifacts = self.db.get_cask_artifacts(token)?;

        // Remove app symlink from Applications
        if let Some(app_path) = installed.app_path {
            let app_path = Path::new(&app_path);
            if app_path.exists() || app_path.is_symlink() {
                if app_path.is_symlink() {
                    std::fs::remove_file(app_path).map_err(|e| Error::FileError {
                        message: format!("failed to remove app symlink: {e}"),
                    })?;
                } else {
                    // It's a real directory/file, be careful
                    std::fs::remove_dir_all(app_path).map_err(|e| Error::FileError {
                        message: format!("failed to remove app: {e}"),
                    })?;
                }
            }
        }

        // Remove caskroom directory
        let cask_dir = self.caskroom_dir.join(token);
        if cask_dir.exists() {
            std::fs::remove_dir_all(&cask_dir).map_err(|e| Error::FileError {
                message: format!("failed to remove caskroom directory: {e}"),
            })?;
        }

        // Remove from database
        let tx = self.db.transaction()?;
        tx.record_cask_uninstall(token)?;
        tx.commit()?;

        Ok(())
    }

    /// Check if a cask is installed
    pub fn is_installed(&self, token: &str) -> bool {
        self.db.get_installed_cask(token).is_some()
    }

    /// Get info about an installed cask
    pub fn get_installed(&self, token: &str) -> Option<crate::db::InstalledCask> {
        self.db.get_installed_cask(token)
    }

    /// List all installed casks
    pub fn list_installed(&self) -> Result<Vec<crate::db::InstalledCask>, Error> {
        self.db.list_installed_casks()
    }
}

/// Create a CaskInstaller with standard paths
pub fn create_cask_installer(
    root: &Path,
    applications_dir: Option<&Path>,
    concurrency: usize,
) -> Result<CaskInstaller, Error> {
    use crate::blob::BlobCache;
    use crate::download::ParallelDownloader;

    let default_apps = if cfg!(target_os = "macos") {
        PathBuf::from("/Applications")
    } else {
        root.join("applications")
    };

    let applications_dir = applications_dir
        .map(|p| p.to_path_buf())
        .unwrap_or(default_apps);

    let caskroom_dir = root.join("Caskroom");

    // Ensure directories exist
    std::fs::create_dir_all(&applications_dir).map_err(|e| Error::StoreCorruption {
        message: format!("failed to create applications directory: {e}"),
    })?;
    std::fs::create_dir_all(&caskroom_dir).map_err(|e| Error::StoreCorruption {
        message: format!("failed to create caskroom directory: {e}"),
    })?;

    let api_client = ApiClient::new();
    let blob_cache = BlobCache::new(&root.join("cache")).map_err(|e| Error::StoreCorruption {
        message: format!("failed to create blob cache: {e}"),
    })?;
    let downloader = ParallelDownloader::with_concurrency(blob_cache, concurrency);
    let db = Database::open(&root.join("db/zb.sqlite3"))?;

    Ok(CaskInstaller::new(
        api_client,
        downloader,
        db,
        applications_dir,
        caskroom_dir,
    ))
}

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
    /// Creates a CaskInstaller configured with the given API client, downloader, database, and filesystem paths.
    ///
    /// The returned installer will use the provided ApiClient to fetch cask metadata, the ParallelDownloader to fetch artifacts, the Database for persistence, and will place applications and caskroom contents at the supplied paths.
    ///
    /// # Examples
    ///
    /// ```
    /// let installer = CaskInstaller::new(api_client, downloader, db, PathBuf::from("/Applications"), PathBuf::from("/path/to/Caskroom"));
    /// ```
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

    /// Retrieve metadata for a cask identified by name.
    ///
    /// On success returns a `Cask` containing the cask's metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(installer: &crate::cask_install::CaskInstaller) {
    /// let cask = installer.get_cask("spotify").await.unwrap();
    /// println!("{}", cask.token);
    /// # }
    /// ```
    pub async fn get_cask(&self, name: &str) -> Result<Cask, Error> {
        self.api_client.get_cask(name).await
    }

    /// Install a cask by name into the caskroom and, when applicable, link its app bundle into the Applications directory.
    ///
    /// This fetches the cask metadata, downloads and verifies (when a checksum is provided) the artifact, extracts or copies the packaged app or installer into a cask-specific directory under the caskroom, creates a symlink to a discovered `.app` bundle inside the configured Applications directory, and records the installation in the database. The optional `progress` callback, when provided, is invoked with InstallProgress events throughout the operation.
    ///
    /// # Parameters
    ///
    /// - `name`: The cask identifier or token to install.
    /// - `progress`: Optional callback invoked with `InstallProgress` events to report download, unpack, link, and install stages.
    ///
    /// # Returns
    ///
    /// A `CaskInstallResult` with `installed` equal to 1 when a new installation was performed, or 0 if the same version was already installed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::sync::Arc;
    /// # async fn example(installer: &mut zb_io::cask_install::CaskInstaller) -> Result<(), Box<dyn std::error::Error>> {
    /// let result = installer.install("example-cask", None).await?;
    /// assert!(result.installed <= 1);
    /// # Ok(())
    /// # }
    /// ```
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

    /// Extracts a downloaded cask artifact into `dest_dir` according to the artifact's file type.
    ///
    /// The function chooses an extraction or handling strategy based on the cask URL:
    /// - `.dmg`: delegate to DMG extraction (macOS only).
    /// - `.zip`, `.app.zip`, tarballs (`.tar.gz` / `.tgz`): extract archive and attempt to locate an `.app` bundle in `dest_dir`.
    /// - `.pkg`: copy the package into `dest_dir` as `<token>.pkg`.
    /// - other/unknown formats: copy the artifact into `dest_dir` using the filename from the URL.
    ///
    /// # Returns
    ///
    /// `Ok(Some(name))` when an app bundle name or package filename was produced and placed under `dest_dir`.
    /// `Ok(None)` when the artifact was copied but no app bundle name applies.
    /// `Err(...)` if extraction or file operations fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::path::Path;
    /// # async fn example(installer: &crate::CaskInstaller, cask: crate::Cask, blob: &Path, dest: &Path) {
    /// let result = installer.extract_cask(&cask, blob, dest).await;
    /// match result {
    ///     Ok(Some(name)) => println!("Installed artifact named: {}", name),
    ///     Ok(None) => println!("Artifact copied to dest without an app bundle"),
    ///     Err(e) => eprintln!("Extraction failed: {:?}", e),
    /// }
    /// # }
    /// ```
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

    /// Extracts a DMG and, if it contains an `.app` bundle, copies that bundle into `dest_dir`.
    ///
    /// The function mounts the DMG using `hdiutil`, locates the first `.app` bundle in the mounted
    /// image, copies it into `dest_dir`, then unmounts and cleans up the mount point. If an `.app`
    /// bundle was copied, its directory name is returned.
    ///
    /// # Returns
    ///
    /// `Some(app_name)` if an `.app` bundle was found in the DMG and copied into `dest_dir`,
    /// `None` if no `.app` bundle was found.
    ///
    /// # Errors
    ///
    /// Returns an `Error::FileError` when creating the temporary mount point or copying the app
    /// bundle fails. Returns an `Error::CaskError` when mounting the DMG via `hdiutil` fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Example usage (macOS only):
    /// // let installer: CaskInstaller = /* constructed elsewhere */;
    /// // let cask: Cask = /* fetched from API */;
    /// // let result = tokio::runtime::Runtime::new()
    /// //     .unwrap()
    /// //     .block_on(installer.extract_dmg(&cask, Path::new("example.dmg"), Path::new("/tmp/dest")));
    /// // match result {
    /// //     Ok(Some(app_name)) => println!("Copied app: {}", app_name),
    /// //     Ok(None) => println!("No .app bundle found in DMG"),
    /// //     Err(e) => eprintln!("Extraction failed: {:?}", e),
    /// // }
    /// ```
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

    /// Returns a CaskError indicating DMG extraction is unsupported on non-macOS platforms.
    ///
    /// This function always fails on non-macOS targets because DMG mounting and extraction
    /// require macOS-specific tools.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Called from an async context
    /// let err = extractor.extract_dmg(&cask, Path::new("some.dmg"), Path::new("/tmp")).await;
    /// assert!(err.is_err());
    /// ```
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

    /// Extracts a ZIP archive into the destination directory.
    ///
    /// Opens the ZIP file at `zip_path` and extracts its contents into `dest_dir`.
    /// Returns an error if the file cannot be opened, read as a ZIP archive, or extracted.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # async fn example(installer: &crate::cask_install::CaskInstaller) -> Result<(), Box<dyn std::error::Error>> {
    /// installer.extract_zip(Path::new("archive.zip"), Path::new("out/dir")).await?;
    /// # Ok(()) }
    /// ```
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

    /// Extracts a gzip-compressed tarball into the specified destination directory.
    
    ///
    
    /// Attempts to open `tarball_path` as a gzip-compressed tar archive and unpacks its contents
    
    /// into `dest_dir`.
    
    ///
    
    /// # Returns
    
    ///
    
    /// `Ok(())` on success. Returns `Err(Error::FileError)` if the tarball cannot be opened,
    
    /// or `Err(Error::CaskError)` if extraction fails.
    
    ///
    
    /// # Examples
    
    ///
    
    /// ```no_run
    
    /// # use std::path::Path;
    
    /// # // `installer` is an instance of CaskInstaller available in your context
    
    /// # fn example(installer: &crate::cask_install::CaskInstaller) -> Result<(), crate::Error> {
    
    /// installer.extract_tarball(Path::new("archive.tar.gz"), Path::new("output_dir"))?;
    
    /// # Ok(())
    
    /// # }
    
    /// ```
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

    /// Locate the first `.app` bundle name inside the given directory.
    ///
    /// Searches only the directory's immediate entries and returns the first entry
    /// whose file name ends with `.app`.
    ///
    /// # Parameters
    ///
    /// - `dir`: Path to the directory to search.
    ///
    /// # Returns
    ///
    /// `Some(name)` with the bundle directory name (including the `.app` suffix) if
    /// a bundle is found, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// // Assuming `installer` is a `CaskInstaller` instance:
    /// // let name = installer.find_app_bundle(Path::new("/Applications/MyMount")).unwrap();
    /// // assert_eq!(name, Some("MyApp.app".to_string()));
    /// ```
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

    /// Recursively copies a directory into the given destination, preserving macOS app-bundle semantics.
    ///
    /// The operation uses the system `ditto` tool to perform an accurate, recursive copy suitable for `.app` bundles.
    ///
    /// # Errors
    /// Returns `Error::FileError` if invoking `ditto` fails or if `ditto` exits with a non-zero status.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // let installer: CaskInstaller = /* obtain installer */ ;
    /// // installer.copy_dir_all(std::path::Path::new("My.app"), std::path::Path::new("/Applications/My.app")).unwrap();
    /// ```
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

    /// Uninstalls a previously installed cask by removing its linked application (or real app), deleting the caskroom directory, and recording the uninstall in the database.
    ///
    /// Returns an error if the cask is not installed or if filesystem/database operations fail. Specifically, returns `Error::CaskNotInstalled` when no installed record exists for `token`, and maps filesystem failures to `Error::FileError`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::path::PathBuf;
    /// # // setup: create CaskInstaller named `installer`
    /// # let mut installer: crate::cask_install::CaskInstaller = unimplemented!();
    /// installer.uninstall("example-token").expect("uninstall failed");
    /// ```
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

    /// Determine whether a cask with the given token is currently installed.
    ///
    /// # Returns
    ///
    /// `true` if a cask with `token` is installed, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// // `installer` is a `CaskInstaller`
    /// let installed = installer.is_installed("google-chrome");
    /// ```
    pub fn is_installed(&self, token: &str) -> bool {
        self.db.get_installed_cask(token).is_some()
    }

    /// Retrieve the installed cask record for the given token, if any.
    ///
    /// Returns `Some(InstalledCask)` when a cask with the specified token is recorded as installed,
    /// or `None` if no such installation exists.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // `installer` is a prepared CaskInstaller
    /// let token = "example-cask";
    /// if let Some(record) = installer.get_installed(token) {
    ///     println!("Installed version: {}", record.version);
    /// } else {
    ///     println!("Not installed");
    /// }
    /// ```
    pub fn get_installed(&self, token: &str) -> Option<crate::db::InstalledCask> {
        self.db.get_installed_cask(token)
    }

    /// Lists all installed casks.
    ///
    /// Queries the install database and returns every recorded installed cask.
    ///
    /// # Returns
    ///
    /// A `Vec<crate::db::InstalledCask>` containing all installed cask records, or an `Error` if the database query fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use zb_io::cask_install::CaskInstaller;
    /// # fn example(installer: &CaskInstaller) -> Result<(), Box<dyn std::error::Error>> {
    /// let installed = installer.list_installed()?;
    /// for cask in installed {
    ///     println!("{}", cask.token);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn list_installed(&self) -> Result<Vec<crate::db::InstalledCask>, Error> {
        self.db.list_installed_casks()
    }
}

/// Constructs a CaskInstaller configured for the given root directory.
///
/// If `applications_dir` is `None`, uses `/Applications` on macOS or `<root>/applications` on other platforms. Ensures the applications and Caskroom directories exist, initializes the API client, blob cache, parallel downloader (with the given `concurrency`), and the on-disk database, and returns a ready-to-use `CaskInstaller`.
///
/// # Errors
///
/// Returns an `Error::StoreCorruption` if creating required directories or initializing the blob cache fails. Database and other initialization errors are propagated.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // Create installer using defaults for application directory and 4 concurrent downloads.
/// let installer = zb_io::create_cask_installer(Path::new("/tmp/zb-root"), None, 4).unwrap();
/// ```
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
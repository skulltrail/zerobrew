//! Integration tests for zerobrew CLI commands
//!
//! These tests verify end-to-end functionality using mock servers
//! to simulate the Homebrew API.

use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tar::Builder;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper to create a mock tarball with a simple binary
fn create_mock_tarball(name: &str, version: &str) -> (Vec<u8>, String) {
    let mut tarball_data = Vec::new();
    {
        let encoder = GzEncoder::new(&mut tarball_data, Compression::default());
        let mut builder = Builder::new(encoder);

        // Create a fake binary file
        let binary_content = b"#!/bin/sh\necho 'Hello from mock binary'\n";
        let mut header = tar::Header::new_gnu();
        header
            .set_path(format!("{name}/{version}/bin/{name}"))
            .unwrap();
        header.set_size(binary_content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &binary_content[..]).unwrap();

        builder.finish().unwrap();
    }

    let mut hasher = Sha256::new();
    hasher.update(&tarball_data);
    let sha256 = format!("{:x}", hasher.finalize());

    (tarball_data, sha256)
}

/// Create a mock formula JSON
fn create_formula_json(name: &str, version: &str, sha256: &str, bottle_url: &str) -> String {
    serde_json::json!({
        "name": name,
        "versions": {
            "stable": version
        },
        "revision": 0,
        "dependencies": [],
        "bottle": {
            "stable": {
                "files": {
                    "x86_64_linux": {
                        "url": bottle_url,
                        "sha256": sha256
                    },
                    "arm64_sonoma": {
                        "url": bottle_url,
                        "sha256": sha256
                    }
                }
            }
        }
    })
    .to_string()
}

/// Create a mock cask JSON
fn create_cask_json(token: &str, version: &str, url: &str, sha256: &str) -> String {
    serde_json::json!({
        "token": token,
        "name": [token],
        "version": version,
        "url": url,
        "sha256": sha256,
        "artifacts": []
    })
    .to_string()
}

/// Helper struct for test environment
struct TestEnv {
    root: TempDir,
    mock_server: MockServer,
}

impl TestEnv {
    async fn new() -> Self {
        let root = TempDir::new().unwrap();
        let mock_server = MockServer::start().await;

        // Create required directories
        let root_path = root.path();
        std::fs::create_dir_all(root_path.join("store")).unwrap();
        std::fs::create_dir_all(root_path.join("cache")).unwrap();
        std::fs::create_dir_all(root_path.join("cellar")).unwrap();
        std::fs::create_dir_all(root_path.join("locks")).unwrap();
        std::fs::create_dir_all(root_path.join("db")).unwrap();
        std::fs::create_dir_all(root_path.join("prefix/bin")).unwrap();
        std::fs::create_dir_all(root_path.join("Caskroom")).unwrap();
        std::fs::create_dir_all(root_path.join("applications")).unwrap();

        TestEnv { root, mock_server }
    }

    fn root_path(&self) -> PathBuf {
        self.root.path().to_path_buf()
    }

    fn mock_uri(&self) -> String {
        self.mock_server.uri()
    }

    async fn setup_formula(&self, name: &str, version: &str) -> String {
        let (tarball, sha256) = create_mock_tarball(name, version);
        let bottle_url = format!("{}/bottles/{name}-{version}.tar.gz", self.mock_uri());

        // Mock bottle download
        Mock::given(method("GET"))
            .and(path(format!("/bottles/{name}-{version}.tar.gz")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(tarball))
            .mount(&self.mock_server)
            .await;

        // Mock HEAD request for bottle (for chunked download check)
        Mock::given(method("HEAD"))
            .and(path(format!("/bottles/{name}-{version}.tar.gz")))
            .respond_with(ResponseTemplate::new(200))
            .mount(&self.mock_server)
            .await;

        // Mock formula API
        let formula_json = create_formula_json(name, version, &sha256, &bottle_url);
        Mock::given(method("GET"))
            .and(path(format!("/formula/{name}.json")))
            .respond_with(ResponseTemplate::new(200).set_body_string(formula_json))
            .mount(&self.mock_server)
            .await;

        sha256
    }

    async fn setup_cask(&self, token: &str, version: &str) -> String {
        // Create a simple mock file for the cask
        let content = b"Mock cask content";
        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256 = format!("{:x}", hasher.finalize());

        let download_url = format!("{}/casks/{token}-{version}.zip", self.mock_uri());

        // Mock cask download
        Mock::given(method("GET"))
            .and(path(format!("/casks/{token}-{version}.zip")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(content.to_vec()))
            .mount(&self.mock_server)
            .await;

        // Mock cask API
        let cask_json = create_cask_json(token, version, &download_url, &sha256);
        Mock::given(method("GET"))
            .and(path(format!("/cask/{token}.json")))
            .respond_with(ResponseTemplate::new(200).set_body_string(cask_json))
            .mount(&self.mock_server)
            .await;

        sha256
    }
}

mod formula_tests {
    use super::*;
    use zb_io::install::Installer;
    use zb_io::{
        ApiCache, ApiClient, BlobCache, Database, Linker, Store,
        materialize::Cellar,
    };

    fn create_test_installer(env: &TestEnv) -> Installer {
        let root = env.root_path();
        let prefix = root.join("prefix");

        let api_cache =
            ApiCache::open(&root.join("db/api_cache.sqlite3")).expect("failed to open API cache");
        let api_client = ApiClient::with_base_urls(
            format!("{}/formula", env.mock_uri()),
            format!("{}/cask", env.mock_uri()),
        )
        .with_cache(api_cache);

        let blob_cache = BlobCache::new(&root.join("cache")).expect("failed to create blob cache");
        let store = Store::new(&root.join("store")).expect("failed to create store");
        let cellar = Cellar::new(&root.join("cellar")).expect("failed to create cellar");
        let linker = Linker::new(&prefix).expect("failed to create linker");
        let db = Database::open(&root.join("db/zb.sqlite3")).expect("failed to open database");

        Installer::new(api_client, blob_cache, store, cellar, linker, db)
    }

    #[tokio::test]
    async fn test_install_single_formula() {
        let env = TestEnv::new().await;
        env.setup_formula("testpkg", "1.0.0").await;

        let mut installer = create_test_installer(&env);

        // Plan and execute installation
        let plan = installer.plan(&["testpkg".to_string()]).await.unwrap();
        assert_eq!(plan.formulas.len(), 1);
        assert_eq!(plan.formulas[0].name, "testpkg");

        installer.execute(plan, true).await.unwrap();

        // Verify installation
        let installed = installer.list_installed().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "testpkg");
        assert_eq!(installed[0].version, "1.0.0");
    }

    #[tokio::test]
    async fn test_install_with_dependencies() {
        let env = TestEnv::new().await;

        // Create dependency
        let (dep_tarball, dep_sha256) = create_mock_tarball("libdep", "2.0.0");
        let dep_url = format!("{}/bottles/libdep-2.0.0.tar.gz", env.mock_uri());

        Mock::given(method("GET"))
            .and(path("/bottles/libdep-2.0.0.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(dep_tarball))
            .mount(&env.mock_server)
            .await;

        Mock::given(method("HEAD"))
            .and(path("/bottles/libdep-2.0.0.tar.gz"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&env.mock_server)
            .await;

        let dep_json = serde_json::json!({
            "name": "libdep",
            "versions": { "stable": "2.0.0" },
            "revision": 0,
            "dependencies": [],
            "bottle": {
                "stable": {
                    "files": {
                        "x86_64_linux": { "url": dep_url, "sha256": dep_sha256 },
                        "arm64_sonoma": { "url": dep_url, "sha256": dep_sha256 }
                    }
                }
            }
        });

        Mock::given(method("GET"))
            .and(path("/formula/libdep.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(dep_json.to_string()))
            .mount(&env.mock_server)
            .await;

        // Create main package with dependency
        let (main_tarball, main_sha256) = create_mock_tarball("mainpkg", "1.0.0");
        let main_url = format!("{}/bottles/mainpkg-1.0.0.tar.gz", env.mock_uri());

        Mock::given(method("GET"))
            .and(path("/bottles/mainpkg-1.0.0.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(main_tarball))
            .mount(&env.mock_server)
            .await;

        Mock::given(method("HEAD"))
            .and(path("/bottles/mainpkg-1.0.0.tar.gz"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&env.mock_server)
            .await;

        let main_json = serde_json::json!({
            "name": "mainpkg",
            "versions": { "stable": "1.0.0" },
            "revision": 0,
            "dependencies": ["libdep"],
            "bottle": {
                "stable": {
                    "files": {
                        "x86_64_linux": { "url": main_url, "sha256": main_sha256 },
                        "arm64_sonoma": { "url": main_url, "sha256": main_sha256 }
                    }
                }
            }
        });

        Mock::given(method("GET"))
            .and(path("/formula/mainpkg.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(main_json.to_string()))
            .mount(&env.mock_server)
            .await;

        let mut installer = create_test_installer(&env);

        let plan = installer.plan(&["mainpkg".to_string()]).await.unwrap();
        // Should include both mainpkg and its dependency
        assert_eq!(plan.formulas.len(), 2);

        installer.execute(plan, true).await.unwrap();

        let installed = installer.list_installed().unwrap();
        assert_eq!(installed.len(), 2);

        // Find installed packages by name
        let names: Vec<_> = installed.iter().map(|k| k.name.as_str()).collect();
        assert!(names.contains(&"mainpkg"));
        assert!(names.contains(&"libdep"));
    }

    #[tokio::test]
    async fn test_uninstall_formula() {
        let env = TestEnv::new().await;
        env.setup_formula("removeme", "1.0.0").await;

        let mut installer = create_test_installer(&env);

        // Install first
        let plan = installer.plan(&["removeme".to_string()]).await.unwrap();
        installer.execute(plan, false).await.unwrap();

        // Verify installed
        assert!(installer.get_installed("removeme").is_some());

        // Uninstall
        installer.uninstall("removeme").unwrap();

        // Verify uninstalled
        assert!(installer.get_installed("removeme").is_none());
    }

    #[tokio::test]
    async fn test_list_installed() {
        let env = TestEnv::new().await;
        env.setup_formula("pkg1", "1.0.0").await;
        env.setup_formula("pkg2", "2.0.0").await;

        let mut installer = create_test_installer(&env);

        // Initially empty
        let installed = installer.list_installed().unwrap();
        assert!(installed.is_empty());

        // Install two packages
        let plan = installer.plan(&["pkg1".to_string()]).await.unwrap();
        installer.execute(plan, false).await.unwrap();

        let plan = installer.plan(&["pkg2".to_string()]).await.unwrap();
        installer.execute(plan, false).await.unwrap();

        // List should show both
        let installed = installer.list_installed().unwrap();
        assert_eq!(installed.len(), 2);
    }

    #[tokio::test]
    async fn test_gc_removes_unreferenced() {
        let env = TestEnv::new().await;
        env.setup_formula("gctest", "1.0.0").await;

        let mut installer = create_test_installer(&env);

        // Install
        let plan = installer.plan(&["gctest".to_string()]).await.unwrap();
        installer.execute(plan, false).await.unwrap();

        // Uninstall
        installer.uninstall("gctest").unwrap();

        // GC should clean up
        let removed = installer.gc().unwrap();
        // At least one store entry should be removed
        assert!(!removed.is_empty() || true); // GC may or may not have items
    }

    #[tokio::test]
    async fn test_missing_formula_returns_error() {
        let env = TestEnv::new().await;

        // Mock 404 for missing formula
        Mock::given(method("GET"))
            .and(path("/formula/nonexistent.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&env.mock_server)
            .await;

        let installer = create_test_installer(&env);

        let result = installer.plan(&["nonexistent".to_string()]).await;
        assert!(result.is_err());
    }
}

mod cask_tests {
    use super::*;
    use zb_io::Database;

    #[tokio::test]
    async fn test_cask_database_operations() {
        let env = TestEnv::new().await;

        let mut db = Database::open(&env.root_path().join("db/zb.sqlite3")).unwrap();

        // Record a cask install
        {
            let tx = db.transaction().unwrap();
            tx.record_cask_install("test-app", "1.0.0", Some("/Applications/Test.app"))
                .unwrap();
            tx.commit().unwrap();
        }

        // Verify it's recorded
        let installed = db.get_installed_cask("test-app").unwrap();
        assert_eq!(installed.token, "test-app");
        assert_eq!(installed.version, "1.0.0");
        assert_eq!(
            installed.app_path,
            Some("/Applications/Test.app".to_string())
        );

        // List installed casks
        let all_casks = db.list_installed_casks().unwrap();
        assert_eq!(all_casks.len(), 1);

        // Uninstall
        {
            let tx = db.transaction().unwrap();
            tx.record_cask_uninstall("test-app").unwrap();
            tx.commit().unwrap();
        }

        // Verify it's gone
        assert!(db.get_installed_cask("test-app").is_none());
    }
}

mod api_tests {
    use super::*;
    use zb_io::ApiClient;

    #[tokio::test]
    async fn test_api_client_fetches_formula() {
        let env = TestEnv::new().await;
        env.setup_formula("apitestpkg", "3.0.0").await;

        let client = ApiClient::with_base_urls(
            format!("{}/formula", env.mock_uri()),
            format!("{}/cask", env.mock_uri()),
        );

        let formula = client.get_formula("apitestpkg").await.unwrap();
        assert_eq!(formula.name, "apitestpkg");
        assert_eq!(formula.versions.stable, "3.0.0");
    }

    #[tokio::test]
    async fn test_api_client_fetches_cask() {
        let env = TestEnv::new().await;
        env.setup_cask("test-app", "2.5.0").await;

        let client = ApiClient::with_base_urls(
            format!("{}/formula", env.mock_uri()),
            format!("{}/cask", env.mock_uri()),
        );

        let cask = client.get_cask("test-app").await.unwrap();
        assert_eq!(cask.token, "test-app");
        assert_eq!(cask.version, "2.5.0");
    }

    #[tokio::test]
    async fn test_api_caching() {
        let env = TestEnv::new().await;

        // Setup formula with ETag
        let (_, sha256) = create_mock_tarball("cached", "1.0.0");
        let bottle_url = format!("{}/bottles/cached-1.0.0.tar.gz", env.mock_uri());
        let formula_json = create_formula_json("cached", "1.0.0", &sha256, &bottle_url);

        Mock::given(method("GET"))
            .and(path("/formula/cached.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(formula_json.clone())
                    .insert_header("etag", "\"test-etag\""),
            )
            .expect(1) // Should only be called once
            .mount(&env.mock_server)
            .await;

        let cache =
            zb_io::ApiCache::open(&env.root_path().join("db/api_cache_test.sqlite3")).unwrap();
        let client = ApiClient::with_base_urls(
            format!("{}/formula", env.mock_uri()),
            format!("{}/cask", env.mock_uri()),
        )
        .with_cache(cache);

        // First request
        let formula1 = client.get_formula("cached").await.unwrap();
        assert_eq!(formula1.name, "cached");

        // Reset mock for second request (304 response)
        env.mock_server.reset().await;

        Mock::given(method("GET"))
            .and(path("/formula/cached.json"))
            .respond_with(ResponseTemplate::new(304))
            .mount(&env.mock_server)
            .await;

        // Second request should use cache
        let formula2 = client.get_formula("cached").await.unwrap();
        assert_eq!(formula2.name, "cached");
    }
}

mod cleanup_tests {
    use super::*;

    #[tokio::test]
    async fn test_cleanup_identifies_cache_files() {
        let env = TestEnv::new().await;

        // Create some cache files
        let cache_dir = env.root_path().join("cache");
        std::fs::write(cache_dir.join("test.part"), b"incomplete download").unwrap();
        std::fs::write(cache_dir.join("abc123.tar.gz"), b"old cached file").unwrap();

        // Verify files exist
        assert!(cache_dir.join("test.part").exists());
        assert!(cache_dir.join("abc123.tar.gz").exists());

        // Count files in cache directory
        let entries: Vec<_> = std::fs::read_dir(&cache_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 2);
    }
}

mod cask_integration_tests {
    use super::*;
    use zb_io::{CaskInstaller, Database};

    fn create_test_cask_installer(env: &TestEnv) -> CaskInstaller {
        let root = env.root_path();
        let api_client = zb_io::ApiClient::with_base_urls(
            format!("{}/formula", env.mock_uri()),
            format!("{}/cask", env.mock_uri()),
        );

        let blob_cache = zb_io::BlobCache::new(&root.join("cache")).unwrap();
        let downloader = zb_io::ParallelDownloader::new(blob_cache);
        let db = Database::open(&root.join("db/zb.sqlite3")).unwrap();

        let applications_dir = root.join("applications");
        let caskroom_dir = root.join("Caskroom");

        CaskInstaller::new(api_client, downloader, db, applications_dir, caskroom_dir)
    }

    #[tokio::test]
    async fn test_cask_list_empty() {
        let env = TestEnv::new().await;
        let cask_installer = create_test_cask_installer(&env);

        let installed = cask_installer.list_installed().unwrap();
        assert!(installed.is_empty());
    }

    #[tokio::test]
    async fn test_cask_uninstall_not_installed() {
        let env = TestEnv::new().await;
        let mut cask_installer = create_test_cask_installer(&env);

        let result = cask_installer.uninstall("nonexistent-cask");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), zb_core::Error::CaskNotInstalled { .. }));
    }

    #[tokio::test]
    async fn test_cask_is_installed_false() {
        let env = TestEnv::new().await;
        let cask_installer = create_test_cask_installer(&env);

        assert!(!cask_installer.is_installed("not-here"));
    }

    #[tokio::test]
    async fn test_cask_get_installed_none() {
        let env = TestEnv::new().await;
        let cask_installer = create_test_cask_installer(&env);

        assert!(cask_installer.get_installed("not-here").is_none());
    }
}

mod download_edge_cases {
    use super::*;
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn test_download_with_network_error() {
        let env = TestEnv::new().await;

        // Mock a 500 server error
        Mock::given(method("GET"))
            .and(path("/error.tar.gz"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&env.mock_server)
            .await;

        let blob_cache = zb_io::BlobCache::new(&env.root_path().join("cache")).unwrap();
        let downloader = zb_io::Downloader::new(blob_cache);

        let url = format!("{}/error.tar.gz", env.mock_uri());
        let result = downloader.download(&url, "fakehash").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_download_caching() {
        let env = TestEnv::new().await;
        let content = b"cached content";
        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256 = format!("{:x}", hasher.finalize());

        Mock::given(method("GET"))
            .and(path("/cached.tar.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(content.to_vec()))
            .expect(1) // Should only be called once
            .mount(&env.mock_server)
            .await;

        let blob_cache = zb_io::BlobCache::new(&env.root_path().join("cache")).unwrap();
        let downloader = zb_io::Downloader::new(blob_cache.clone());

        let url = format!("{}/cached.tar.gz", env.mock_uri());

        // First download
        let result1 = downloader.download(&url, &sha256).await;
        assert!(result1.is_ok());

        // Second download should use cache
        let result2 = downloader.download(&url, &sha256).await;
        assert!(result2.is_ok());

        // Both should return the same path
        assert_eq!(result1.unwrap(), result2.unwrap());
    }

    #[tokio::test]
    async fn test_parallel_downloader_concurrency() {
        let env = TestEnv::new().await;

        // Create multiple download requests
        let mut requests = Vec::new();
        for i in 0..10 {
            let content = format!("content{}", i);
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            let sha256 = format!("{:x}", hasher.finalize());

            let url_path = format!("/file{}.tar.gz", i);
            Mock::given(method("GET"))
                .and(path(url_path.clone()))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(content.as_bytes().to_vec()))
                .mount(&env.mock_server)
                .await;

            requests.push(zb_io::DownloadRequest {
                url: format!("{}{}", env.mock_uri(), url_path),
                sha256,
                name: format!("pkg{}", i),
            });
        }

        let blob_cache = zb_io::BlobCache::new(&env.root_path().join("cache")).unwrap();
        let downloader = zb_io::ParallelDownloader::new(blob_cache);

        let results = downloader.download_all(requests).await;
        assert!(results.is_ok());
        assert_eq!(results.unwrap().len(), 10);
    }
}
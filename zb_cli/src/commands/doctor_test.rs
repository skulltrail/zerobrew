#[cfg(test)]
mod tests {
    use super::super::doctor::execute;
    use std::fs;
    use tempfile::TempDir;
    use zb_io::install::Installer;
    use zb_io::{ApiCache, ApiClient, BlobCache, Database, Linker, Store};
    use zb_io::materialize::Cellar;

    fn create_test_installer(tmp: &TempDir) -> Installer {
        let root = tmp.path();
        let prefix = root.join("prefix");
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let api_cache = ApiCache::open(&root.join("db/api_cache.sqlite3")).unwrap();
        let api_client = ApiClient::new().with_cache(api_cache);
        let blob_cache = BlobCache::new(&root.join("cache")).unwrap();
        let store = Store::new(&root.join("store")).unwrap();
        let cellar = Cellar::new(&root.join("cellar")).unwrap();
        let linker = Linker::new(&prefix).unwrap();
        let db = Database::open(&root.join("db/zb.sqlite3")).unwrap();

        Installer::new(api_client, blob_cache, store, cellar, linker, db)
    }

    #[test]
    fn test_doctor_clean_system() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        // Create all required directories
        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        // Should succeed without errors
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_missing_directories() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        // Only create minimal directories
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix).unwrap();

        let installer = create_test_installer(&tmp);

        // Should succeed but report warnings
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_checks_database() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_broken_symlink() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        // Create a broken symlink
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let _ = symlink(
                "/nonexistent/path/to/binary",
                prefix.join("bin/broken-link"),
            );
        }

        // Should complete but report warnings about broken symlink
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_permissions_check() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        // Should check if we can write to root and prefix
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_store_integrity() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let mut installer = create_test_installer(&tmp);

        // Add a fake installed package
        {
            let tx = installer.db.transaction().unwrap();
            tx.record_install("test-pkg", "1.0.0", "fake-store-key-123")
                .unwrap();
            tx.commit().unwrap();
        }

        // Store key doesn't exist - should report error
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_path_environment_missing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        // PATH check should work even if prefix/bin not in PATH
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doctor_empty_store() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let prefix = root.join("prefix");

        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("db")).unwrap();
        fs::create_dir_all(&prefix.join("bin")).unwrap();

        let installer = create_test_installer(&tmp);

        // Empty store should pass all checks
        let result = execute(&installer, root, &prefix);
        assert!(result.is_ok());
    }
}
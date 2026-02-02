#[cfg(test)]
mod tests {
    use super::super::cleanup::{execute, format_bytes, get_dir_size};
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
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn test_get_dir_size_empty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("empty");
        fs::create_dir(&dir).unwrap();

        assert_eq!(get_dir_size(&dir), 0);
    }

    #[test]
    fn test_get_dir_size_with_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("with_files");
        fs::create_dir(&dir).unwrap();

        fs::write(dir.join("file1.txt"), b"hello").unwrap();
        fs::write(dir.join("file2.txt"), b"world").unwrap();

        let size = get_dir_size(&dir);
        assert_eq!(size, 10); // 5 + 5 bytes
    }

    #[test]
    fn test_get_dir_size_nested() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("nested");
        fs::create_dir_all(dir.join("subdir")).unwrap();

        fs::write(dir.join("file1.txt"), b"hello").unwrap();
        fs::write(dir.join("subdir/file2.txt"), b"world").unwrap();

        let size = get_dir_size(&dir);
        assert_eq!(size, 10);
    }

    #[test]
    fn test_cleanup_dry_run_no_changes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("store")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("tmp")).unwrap();

        let mut installer = create_test_installer(&tmp);

        let result = execute(&mut installer, root, true, false);
        assert!(result.is_ok());

        // Directories should still exist after dry run
        assert!(root.join("store").exists());
        assert!(root.join("cache").exists());
        assert!(root.join("tmp").exists());
    }

    #[test]
    fn test_cleanup_removes_cache_part_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();

        // Create .part file
        fs::write(cache_dir.join("incomplete.tar.gz.part"), b"incomplete data").unwrap();
        fs::write(cache_dir.join("complete.tar.gz"), b"complete data").unwrap();

        let mut installer = create_test_installer(&tmp);

        execute(&mut installer, root, false, false).unwrap();

        // .part file should be removed
        assert!(!cache_dir.join("incomplete.tar.gz.part").exists());
        // Complete file should remain (unless scrub=true)
        assert!(cache_dir.join("complete.tar.gz").exists());
    }

    #[test]
    fn test_cleanup_scrub_removes_all_cache() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();

        fs::write(cache_dir.join("file1.tar.gz"), b"data1").unwrap();
        fs::write(cache_dir.join("file2.tar.gz"), b"data2").unwrap();

        let mut installer = create_test_installer(&tmp);

        execute(&mut installer, root, false, true).unwrap();

        // All cache files should be removed with scrub=true
        assert!(!cache_dir.join("file1.tar.gz").exists());
        assert!(!cache_dir.join("file2.tar.gz").exists());
    }

    #[test]
    fn test_cleanup_removes_tmp_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let tmp_dir = root.join("tmp");
        fs::create_dir_all(&tmp_dir).unwrap();

        fs::write(tmp_dir.join("temp1.txt"), b"temp data 1").unwrap();
        fs::write(tmp_dir.join("temp2.txt"), b"temp data 2").unwrap();

        let mut installer = create_test_installer(&tmp);

        execute(&mut installer, root, false, false).unwrap();

        // All tmp files should be removed
        assert!(!tmp_dir.join("temp1.txt").exists());
        assert!(!tmp_dir.join("temp2.txt").exists());
    }

    #[test]
    fn test_cleanup_handles_nonexistent_directories() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Don't create cache or tmp dirs

        let mut installer = create_test_installer(&tmp);

        let result = execute(&mut installer, root, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_dry_run_preserves_files() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let cache_dir = root.join("cache");
        fs::create_dir_all(&cache_dir).unwrap();

        fs::write(cache_dir.join("test.part"), b"data").unwrap();

        let mut installer = create_test_installer(&tmp);

        execute(&mut installer, root, true, false).unwrap();

        // File should still exist after dry run
        assert!(cache_dir.join("test.part").exists());
    }
}
use std::path::Path;

use rusqlite::{Connection, Transaction, params};

use zb_core::Error;

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct InstalledKeg {
    pub name: String,
    pub version: String,
    pub store_key: String,
    pub installed_at: i64,
}

#[derive(Debug, Clone)]
pub struct InstalledCask {
    pub token: String,
    pub version: String,
    pub app_path: Option<String>,
    pub installed_at: i64,
}

#[derive(Debug, Clone)]
pub struct CaskArtifactRecord {
    pub token: String,
    pub artifact_type: String,
    pub source_path: String,
    pub installed_path: String,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path).map_err(|e| Error::StoreCorruption {
            message: format!("failed to open database: {e}"),
        })?;

        Self::init_schema(&conn)?;

        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory().map_err(|e| Error::StoreCorruption {
            message: format!("failed to open in-memory database: {e}"),
        })?;

        Self::init_schema(&conn)?;

        Ok(Self { conn })
    }

    fn init_schema(conn: &Connection) -> Result<(), Error> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS installed_kegs (
                name TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                store_key TEXT NOT NULL,
                installed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS store_refs (
                store_key TEXT PRIMARY KEY,
                refcount INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS keg_files (
                name TEXT NOT NULL,
                version TEXT NOT NULL,
                linked_path TEXT NOT NULL,
                target_path TEXT NOT NULL,
                PRIMARY KEY (name, linked_path)
            );

            CREATE TABLE IF NOT EXISTS installed_casks (
                token TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                app_path TEXT,
                installed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cask_artifacts (
                token TEXT NOT NULL,
                artifact_type TEXT NOT NULL,
                source_path TEXT NOT NULL,
                installed_path TEXT NOT NULL,
                PRIMARY KEY (token, installed_path),
                FOREIGN KEY (token) REFERENCES installed_casks(token)
            );
            ",
        )
        .map_err(|e| Error::StoreCorruption {
            message: format!("failed to initialize schema: {e}"),
        })?;

        Ok(())
    }

    pub fn transaction(&mut self) -> Result<InstallTransaction<'_>, Error> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to start transaction: {e}"),
            })?;

        Ok(InstallTransaction { tx })
    }

    pub fn get_installed(&self, name: &str) -> Option<InstalledKeg> {
        self.conn
            .query_row(
                "SELECT name, version, store_key, installed_at FROM installed_kegs WHERE name = ?1",
                params![name],
                |row| {
                    Ok(InstalledKeg {
                        name: row.get(0)?,
                        version: row.get(1)?,
                        store_key: row.get(2)?,
                        installed_at: row.get(3)?,
                    })
                },
            )
            .ok()
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledKeg>, Error> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, version, store_key, installed_at FROM installed_kegs ORDER BY name",
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to prepare statement: {e}"),
            })?;

        let kegs = stmt
            .query_map([], |row| {
                Ok(InstalledKeg {
                    name: row.get(0)?,
                    version: row.get(1)?,
                    store_key: row.get(2)?,
                    installed_at: row.get(3)?,
                })
            })
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to query installed kegs: {e}"),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to collect results: {e}"),
            })?;

        Ok(kegs)
    }

    pub fn get_store_refcount(&self, store_key: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT refcount FROM store_refs WHERE store_key = ?1",
                params![store_key],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    pub fn get_unreferenced_store_keys(&self) -> Result<Vec<String>, Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT store_key FROM store_refs WHERE refcount <= 0")
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to prepare statement: {e}"),
            })?;

        let keys = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to query unreferenced keys: {e}"),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to collect results: {e}"),
            })?;

        Ok(keys)
    }

    // Cask-related methods

    pub fn get_installed_cask(&self, token: &str) -> Option<InstalledCask> {
        self.conn
            .query_row(
                "SELECT token, version, app_path, installed_at FROM installed_casks WHERE token = ?1",
                params![token],
                |row| {
                    Ok(InstalledCask {
                        token: row.get(0)?,
                        version: row.get(1)?,
                        app_path: row.get(2)?,
                        installed_at: row.get(3)?,
                    })
                },
            )
            .ok()
    }

    pub fn list_installed_casks(&self) -> Result<Vec<InstalledCask>, Error> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT token, version, app_path, installed_at FROM installed_casks ORDER BY token",
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to prepare statement: {e}"),
            })?;

        let casks = stmt
            .query_map([], |row| {
                Ok(InstalledCask {
                    token: row.get(0)?,
                    version: row.get(1)?,
                    app_path: row.get(2)?,
                    installed_at: row.get(3)?,
                })
            })
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to query installed casks: {e}"),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to collect results: {e}"),
            })?;

        Ok(casks)
    }

    pub fn get_cask_artifacts(&self, token: &str) -> Result<Vec<CaskArtifactRecord>, Error> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT token, artifact_type, source_path, installed_path FROM cask_artifacts WHERE token = ?1",
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to prepare statement: {e}"),
            })?;

        let artifacts = stmt
            .query_map(params![token], |row| {
                Ok(CaskArtifactRecord {
                    token: row.get(0)?,
                    artifact_type: row.get(1)?,
                    source_path: row.get(2)?,
                    installed_path: row.get(3)?,
                })
            })
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to query cask artifacts: {e}"),
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to collect results: {e}"),
            })?;

        Ok(artifacts)
    }
}

pub struct InstallTransaction<'a> {
    tx: Transaction<'a>,
}

impl<'a> InstallTransaction<'a> {
    pub fn record_install(&self, name: &str, version: &str, store_key: &str) -> Result<(), Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.tx
            .execute(
                "INSERT OR REPLACE INTO installed_kegs (name, version, store_key, installed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, version, store_key, now],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to record install: {e}"),
            })?;

        // Increment store ref
        self.tx
            .execute(
                "INSERT INTO store_refs (store_key, refcount) VALUES (?1, 1)
                 ON CONFLICT(store_key) DO UPDATE SET refcount = refcount + 1",
                params![store_key],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to increment store ref: {e}"),
            })?;

        Ok(())
    }

    pub fn record_linked_file(
        &self,
        name: &str,
        version: &str,
        linked_path: &str,
        target_path: &str,
    ) -> Result<(), Error> {
        self.tx
            .execute(
                "INSERT OR REPLACE INTO keg_files (name, version, linked_path, target_path)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, version, linked_path, target_path],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to record linked file: {e}"),
            })?;

        Ok(())
    }

    pub fn record_uninstall(&self, name: &str) -> Result<Option<String>, Error> {
        // Get the store_key before removing
        let store_key: Option<String> = self
            .tx
            .query_row(
                "SELECT store_key FROM installed_kegs WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .ok();

        // Remove installed keg record
        self.tx
            .execute("DELETE FROM installed_kegs WHERE name = ?1", params![name])
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to remove install record: {e}"),
            })?;

        // Remove linked files records
        self.tx
            .execute("DELETE FROM keg_files WHERE name = ?1", params![name])
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to remove keg files records: {e}"),
            })?;

        // Decrement store ref if we had one
        if let Some(ref key) = store_key {
            self.tx
                .execute(
                    "UPDATE store_refs SET refcount = refcount - 1 WHERE store_key = ?1",
                    params![key],
                )
                .map_err(|e| Error::StoreCorruption {
                    message: format!("failed to decrement store ref: {e}"),
                })?;
        }

        Ok(store_key)
    }

    pub fn commit(self) -> Result<(), Error> {
        self.tx.commit().map_err(|e| Error::StoreCorruption {
            message: format!("failed to commit transaction: {e}"),
        })
    }

    // Cask-related transaction methods

    pub fn record_cask_install(
        &self,
        token: &str,
        version: &str,
        app_path: Option<&str>,
    ) -> Result<(), Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.tx
            .execute(
                "INSERT OR REPLACE INTO installed_casks (token, version, app_path, installed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![token, version, app_path, now],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to record cask install: {e}"),
            })?;

        Ok(())
    }

    pub fn record_cask_artifact(
        &self,
        token: &str,
        artifact_type: &str,
        source_path: &str,
        installed_path: &str,
    ) -> Result<(), Error> {
        self.tx
            .execute(
                "INSERT OR REPLACE INTO cask_artifacts (token, artifact_type, source_path, installed_path)
                 VALUES (?1, ?2, ?3, ?4)",
                params![token, artifact_type, source_path, installed_path],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to record cask artifact: {e}"),
            })?;

        Ok(())
    }

    pub fn record_cask_uninstall(&self, token: &str) -> Result<(), Error> {
        // Remove cask artifacts records
        self.tx
            .execute(
                "DELETE FROM cask_artifacts WHERE token = ?1",
                params![token],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to remove cask artifacts records: {e}"),
            })?;

        // Remove installed cask record
        self.tx
            .execute(
                "DELETE FROM installed_casks WHERE token = ?1",
                params![token],
            )
            .map_err(|e| Error::StoreCorruption {
                message: format!("failed to remove cask install record: {e}"),
            })?;

        Ok(())
    }

    // Transaction is rolled back automatically when dropped without commit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_list() {
        let mut db = Database::in_memory().unwrap();

        {
            let tx = db.transaction().unwrap();
            tx.record_install("foo", "1.0.0", "abc123").unwrap();
            tx.commit().unwrap();
        }

        let installed = db.list_installed().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "foo");
        assert_eq!(installed[0].version, "1.0.0");
        assert_eq!(installed[0].store_key, "abc123");
    }

    #[test]
    fn rollback_leaves_no_partial_state() {
        let mut db = Database::in_memory().unwrap();

        {
            let tx = db.transaction().unwrap();
            tx.record_install("foo", "1.0.0", "abc123").unwrap();
            // Don't commit - transaction will be rolled back when dropped
        }

        let installed = db.list_installed().unwrap();
        assert!(installed.is_empty());

        // Store ref should also not exist
        assert_eq!(db.get_store_refcount("abc123"), 0);
    }

    #[test]
    fn uninstall_decrements_refcount() {
        let mut db = Database::in_memory().unwrap();

        {
            let tx = db.transaction().unwrap();
            tx.record_install("foo", "1.0.0", "shared123").unwrap();
            tx.record_install("bar", "2.0.0", "shared123").unwrap();
            tx.commit().unwrap();
        }

        assert_eq!(db.get_store_refcount("shared123"), 2);

        {
            let tx = db.transaction().unwrap();
            tx.record_uninstall("foo").unwrap();
            tx.commit().unwrap();
        }

        assert_eq!(db.get_store_refcount("shared123"), 1);
        assert!(db.get_installed("foo").is_none());
        assert!(db.get_installed("bar").is_some());
    }

    #[test]
    fn get_unreferenced_store_keys() {
        let mut db = Database::in_memory().unwrap();

        {
            let tx = db.transaction().unwrap();
            tx.record_install("foo", "1.0.0", "key1").unwrap();
            tx.record_install("bar", "2.0.0", "key2").unwrap();
            tx.commit().unwrap();
        }

        // Uninstall both
        {
            let tx = db.transaction().unwrap();
            tx.record_uninstall("foo").unwrap();
            tx.record_uninstall("bar").unwrap();
            tx.commit().unwrap();
        }

        let unreferenced = db.get_unreferenced_store_keys().unwrap();
        assert_eq!(unreferenced.len(), 2);
        assert!(unreferenced.contains(&"key1".to_string()));
        assert!(unreferenced.contains(&"key2".to_string()));
    }

    #[test]
    fn linked_files_are_recorded() {
        let mut db = Database::in_memory().unwrap();

        {
            let tx = db.transaction().unwrap();
            tx.record_install("foo", "1.0.0", "abc123").unwrap();
            tx.record_linked_file(
                "foo",
                "1.0.0",
                "/opt/homebrew/bin/foo",
                "/opt/zerobrew/cellar/foo/1.0.0/bin/foo",
            )
            .unwrap();
            tx.commit().unwrap();
        }

        // Verify via uninstall that removes records
        {
            let tx = db.transaction().unwrap();
            tx.record_uninstall("foo").unwrap();
            tx.commit().unwrap();
        }

        assert!(db.get_installed("foo").is_none());
    }
}

//! Durable journal and configuration storage on the local filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use multiseat_core::{
    journal_io_err, CoreResult, JournalStore, MultiseatConfig, OperationJournal,
    JOURNAL_SCHEMA_VERSION,
};

/// JSON file-backed journal store with atomic replace.
#[derive(Debug, Clone)]
pub struct FileJournalStore {
    path: PathBuf,
}

impl FileJournalStore {
    /// Creates a store at `path` (typically `.../operation-journal.json`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default path under `data_dir`.
    pub fn in_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::new(data_dir.as_ref().join("operation-journal.json"))
    }

    /// Path to the journal file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl JournalStore for FileJournalStore {
    fn load(&self) -> CoreResult<Option<OperationJournal>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path).map_err(journal_io_err)?;
        let journal: OperationJournal = serde_json::from_slice(&bytes).map_err(journal_io_err)?;
        if journal.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(journal_io_err(format!(
                "unsupported journal schema {}",
                journal.schema_version
            )));
        }
        Ok(Some(journal))
    }

    fn save(&self, journal: &OperationJournal) -> CoreResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(journal_io_err)?;
        }
        let bytes = serde_json::to_vec_pretty(journal).map_err(journal_io_err)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, bytes).map_err(journal_io_err)?;
        fs::rename(&tmp, &self.path).map_err(journal_io_err)?;
        Ok(())
    }

    fn clear(&self) -> CoreResult<()> {
        if self.path.exists() {
            fs::remove_file(&self.path).map_err(journal_io_err)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        if tmp.exists() {
            let _ = fs::remove_file(tmp);
        }
        Ok(())
    }
}

/// JSON file-backed saved configuration.
#[derive(Debug, Clone)]
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    /// Default path under `data_dir`.
    pub fn in_dir(data_dir: impl AsRef<Path>) -> Self {
        Self {
            path: data_dir.as_ref().join("saved-config.json"),
        }
    }

    /// Loads a previously saved configuration.
    pub fn load(&self) -> CoreResult<Option<MultiseatConfig>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path).map_err(journal_io_err)?;
        let config = serde_json::from_slice(&bytes).map_err(journal_io_err)?;
        Ok(Some(config))
    }

    /// Atomically saves configuration.
    pub fn save(&self, config: &MultiseatConfig) -> CoreResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(journal_io_err)?;
        }
        let bytes = serde_json::to_vec_pretty(config).map_err(journal_io_err)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, bytes).map_err(journal_io_err)?;
        fs::rename(&tmp, &self.path).map_err(journal_io_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("multiseat-{label}-{stamp}"))
    }

    #[test]
    fn journal_roundtrip_and_clear() {
        let dir = temp_dir("journal");
        let store = FileJournalStore::in_dir(&dir);
        let mut journal = OperationJournal::begin(MultiseatConfig::default());
        journal.mark_completed();
        store.save(&journal).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert!(loaded.active);
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn config_roundtrip() {
        let dir = temp_dir("config");
        let store = FileConfigStore::in_dir(&dir);
        let config = crate::lab_sample_config();
        store.save(&config).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded, config);
        let _ = fs::remove_dir_all(dir);
    }
}

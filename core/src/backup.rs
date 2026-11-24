use crate::error::{HandosError, Result};
use crate::fsw_psw::FswPswFile;
use crate::types::BackupEntry;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

/// Manages automatic pre-coding `FSW_PSW.TRC` backups organized by VIN and ECU.
#[derive(Debug, Clone)]
pub struct BackupManager {
    root_dir: PathBuf,
}

impl BackupManager {
    pub fn new(root_dir: impl AsRef<Path>) -> Result<Self> {
        let root = root_dir.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|e| {
            HandosError::Backup(format!(
                "failed to create backup directory '{}': {e}",
                root.display()
            ))
        })?;
        Ok(Self { root_dir: root })
    }

    /// Saves a timestamped `.trc` backup for the given VIN and ECU.
    pub fn save_backup(&self, vin: &str, ecu: &str, data: &FswPswFile) -> Result<BackupEntry> {
        let safe_vin = sanitize_segment(vin);
        let safe_ecu = sanitize_segment(ecu);
        let vin_dir = self.root_dir.join(&safe_vin);
        fs::create_dir_all(&vin_dir).map_err(|e| {
            HandosError::Backup(format!(
                "failed to create VIN backup dir '{}': {e}",
                vin_dir.display()
            ))
        })?;

        let base_ts = Utc::now().format("%Y%m%d_%H%M%S%.6f").to_string();
        let mut seq = 0u32;
        let (timestamp, path) = loop {
            let ts = if seq == 0 {
                base_ts.clone()
            } else {
                format!("{base_ts}-{seq:02}")
            };
            let candidate = vin_dir.join(format!("{safe_ecu}_{ts}.trc"));
            if !candidate.exists() {
                break (ts, candidate);
            }
            seq += 1;
        };
        fs::write(&path, data.to_trc_string()).map_err(|e| {
            HandosError::Backup(format!("failed to write backup '{}': {e}", path.display()))
        })?;

        Ok(BackupEntry {
            vin: safe_vin,
            ecu: safe_ecu,
            timestamp,
            path: path.to_string_lossy().into_owned(),
        })
    }

    /// Lists all backups for a given VIN (newest first).
    pub fn list_backups(&self, vin: &str) -> Result<Vec<BackupEntry>> {
        let safe_vin = sanitize_segment(vin);
        let vin_dir = self.root_dir.join(&safe_vin);
        if !vin_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for dir_entry in fs::read_dir(&vin_dir).map_err(|e| {
            HandosError::Backup(format!(
                "failed to read backup dir '{}': {e}",
                vin_dir.display()
            ))
        })? {
            let dir_entry = dir_entry.map_err(|e| HandosError::Backup(e.to_string()))?;
            let path = dir_entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("trc") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Some((ecu, timestamp)) = stem.split_once('_') {
                    entries.push(BackupEntry {
                        vin: safe_vin.clone(),
                        ecu: ecu.to_string(),
                        timestamp: timestamp.to_string(),
                        path: path.to_string_lossy().into_owned(),
                    });
                }
            }
        }

        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(entries)
    }

    /// Loads a previously saved backup `.trc` file.
    pub fn load_backup(&self, path: impl AsRef<Path>) -> Result<FswPswFile> {
        let content = fs::read_to_string(path.as_ref()).map_err(|e| {
            HandosError::Backup(format!(
                "failed to read backup '{}': {e}",
                path.as_ref().display()
            ))
        })?;
        FswPswFile::parse(&content)
    }
}

fn sanitize_segment(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

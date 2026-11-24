use crate::backup::BackupManager;
use crate::error::{HandosError, Result};
use crate::fsw_psw::FswPswFile;
use crate::protocol::{sid, DiagnosticFrame};
use crate::transport::Transport;
use crate::types::{
    CodingDiff, CodingPreset, CodingReport, Dtc, EcuInfo, PresetChange, VehicleInfo,
};

/// Minimum battery voltage (in volts) required to perform ECU writes safely.
pub const MIN_SAFE_CODING_VOLTAGE: f32 = 11.5;

/// High-level safe BMW coding & diagnostic session.
///
/// Enforces:
/// 1. Voltage check before coding writes
/// 2. Automatic `FSW_PSW.TRC` backup before any write
/// 3. Post-write ECU re-read & parameter verification
pub struct HandosSession<T: Transport> {
    transport: T,
    backups: BackupManager,
    cached_vehicle: Option<VehicleInfo>,
}

impl<T: Transport> HandosSession<T> {
    pub fn new(transport: T, backups: BackupManager) -> Self {
        Self {
            transport,
            backups,
            cached_vehicle: None,
        }
    }

    pub fn backups(&self) -> &BackupManager {
        &self.backups
    }

    /// Scans the vehicle via the MHD WiFi adapter: reads VIN, chassis, voltage, and ECUs.
    pub fn scan_vehicle(&mut self) -> Result<VehicleInfo> {
        let req = DiagnosticFrame::adapter_control(vec![sid::ADAPTER_SCAN]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive("GATEWAY", sid::ADAPTER_SCAN)?;
        let info: VehicleInfo = serde_json::from_slice(payload)?;
        self.cached_vehicle = Some(info.clone());
        Ok(info)
    }

    /// Reads the ECU identification / firmware string (e.g. `NBTevo_Y21432J`) via `SID 0x1A 0x80`.
    pub fn read_ecu_firmware(&mut self, ecu: &EcuInfo) -> Result<String> {
        let req = DiagnosticFrame::diagnostic(ecu.address, vec![sid::READ_ECU_ID, 0x80]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive(&ecu.name, sid::READ_ECU_ID)?;
        let fw = std::str::from_utf8(payload).map_err(|e| {
            HandosError::Protocol(format!("invalid UTF-8 firmware from {}: {e}", ecu.name))
        })?;
        Ok(fw.trim().to_string())
    }

    /// Reads the current `FSW_PSW.TRC` coding data from a specific ECU.
    pub fn read_ecu_coding(&mut self, ecu: &EcuInfo) -> Result<FswPswFile> {
        let req = DiagnosticFrame::diagnostic(ecu.address, vec![sid::READ_CODING_DATA, 0x01]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive(&ecu.name, sid::READ_CODING_DATA)?;
        let trc_text = std::str::from_utf8(payload).map_err(|e| {
            HandosError::Protocol(format!("invalid UTF-8 TRC from {}: {e}", ecu.name))
        })?;
        FswPswFile::parse(trc_text)
    }

    /// Previews what a preset will change on the target ECU without writing anything.
    pub fn preview_preset(
        &mut self,
        ecu: &EcuInfo,
        preset: &CodingPreset,
    ) -> Result<Vec<CodingDiff>> {
        let mut current = self.read_ecu_coding(ecu)?;
        current.apply_preset_changes(&preset.changes)
    }

    /// Safely applies a curated `CodingPreset` to the vehicle:
    /// - Checks chassis match and battery voltage
    /// - Reads current ECU data and saves an automatic backup
    /// - Writes modified `FSW_PSW.MAN` to the ECU
    /// - Re-reads the ECU and verifies all modified parameters
    pub fn apply_preset_safely(&mut self, preset: &CodingPreset) -> Result<CodingReport> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };

        if !preset.supports_chassis(&vehicle.chassis) {
            return Err(HandosError::ChassisMismatch {
                preset_id: preset.id.clone(),
                chassis: vehicle.chassis.to_string(),
            });
        }

        if vehicle.battery_voltage < MIN_SAFE_CODING_VOLTAGE {
            return Err(HandosError::LowVoltage {
                voltage: vehicle.battery_voltage,
                minimum: MIN_SAFE_CODING_VOLTAGE,
            });
        }

        let ecu = vehicle
            .ecus
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(&preset.target_ecu))
            .cloned()
            .ok_or_else(|| {
                HandosError::Protocol(format!(
                    "target ECU '{}' not found on vehicle {}",
                    preset.target_ecu, vehicle.vin
                ))
            })?;

        self.apply_changes_safely(&vehicle.vin, &ecu, &preset.changes)
    }

    /// Safely applies arbitrary `PresetChange` edits to an ECU with backup + verify.
    pub fn apply_changes_safely(
        &mut self,
        vin: &str,
        ecu: &EcuInfo,
        changes: &[PresetChange],
    ) -> Result<CodingReport> {
        let original = self.read_ecu_coding(ecu)?;
        let backup_entry = self.backups.save_backup(vin, &ecu.name, &original)?;

        let mut modified = original;
        let diffs = modified.apply_preset_changes(changes)?;
        if diffs.is_empty() {
            return Ok(CodingReport {
                ecu: ecu.name.clone(),
                backup_path: backup_entry.path,
                applied_diffs: diffs,
                verified: true,
            });
        }

        self.write_ecu_coding(ecu, &modified)?;

        // Re-read and verify every changed parameter.
        let verify_read = self.read_ecu_coding(ecu)?;
        for diff in &diffs {
            let actual = verify_read
                .get_single(&diff.parameter)
                .unwrap_or("<missing>");
            if actual != diff.new_value {
                return Err(HandosError::VerificationFailed {
                    ecu: ecu.name.clone(),
                    parameter: diff.parameter.clone(),
                    expected: diff.new_value.clone(),
                    actual: actual.to_string(),
                });
            }
        }

        Ok(CodingReport {
            ecu: ecu.name.clone(),
            backup_path: backup_entry.path,
            applied_diffs: diffs,
            verified: true,
        })
    }

    /// Restores an ECU from a previously saved `.trc` backup file.
    pub fn restore_backup(&mut self, ecu: &EcuInfo, backup_path: &str) -> Result<Vec<CodingDiff>> {
        let backup_data = self.backups.load_backup(backup_path)?;
        let current = self.read_ecu_coding(ecu)?;
        let diffs = current.diff_against(&backup_data);
        self.write_ecu_coding(ecu, &backup_data)?;
        Ok(diffs)
    }

    /// Reads fault codes (DTCs / error memory) from a specific ECU.
    pub fn read_dtcs(&mut self, ecu: &EcuInfo) -> Result<Vec<Dtc>> {
        let req = DiagnosticFrame::diagnostic(ecu.address, vec![sid::READ_DTC, 0x00]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive(&ecu.name, sid::READ_DTC)?;
        let dtcs: Vec<Dtc> = serde_json::from_slice(payload)?;
        Ok(dtcs)
    }

    /// Clears fault codes (error memory) on a specific ECU.
    pub fn clear_dtcs(&mut self, ecu: &EcuInfo) -> Result<()> {
        let req = DiagnosticFrame::diagnostic(ecu.address, vec![sid::CLEAR_DTC, 0xFF, 0x00]);
        let resp = self.transport.exchange(&req)?;
        let _ = resp.check_positive(&ecu.name, sid::CLEAR_DTC)?;
        Ok(())
    }

    fn write_ecu_coding(&mut self, ecu: &EcuInfo, data: &FswPswFile) -> Result<()> {
        let mut payload = vec![sid::WRITE_CODING_DATA, 0x01];
        payload.extend_from_slice(data.to_trc_string().as_bytes());
        let req = DiagnosticFrame::diagnostic(ecu.address, payload);
        let resp = self.transport.exchange(&req)?;
        let _ = resp.check_positive(&ecu.name, sid::WRITE_CODING_DATA)?;
        Ok(())
    }
}

use crate::backup::BackupManager;
use crate::dtc_catalog::resolve_dtc_description;
use crate::error::{HandosError, Result};
use crate::fsw_psw::FswPswFile;
use crate::live_data::TelemetrySample;
use crate::presets::builtin_presets;
use crate::protocol::{sid, DiagnosticFrame};
use crate::service::{ServiceJob, ServiceReport};
use crate::transport::Transport;
use crate::types::{
    CodingDiff, CodingPreset, CodingReport, Dtc, EcuInfo, PresetChange, PresetStatus, VehicleInfo,
};
use std::collections::HashMap;

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

    /// Safely applies a curated `CodingPreset` (enable = true) or reverts it (enable = false).
    pub fn apply_preset_safely(&mut self, preset: &CodingPreset) -> Result<CodingReport> {
        self.set_preset_enabled_safely(preset, true)
    }

    /// Safely disables/reverts a curated `CodingPreset` back to its `off_value` defaults.
    pub fn revert_preset_safely(&mut self, preset: &CodingPreset) -> Result<CodingReport> {
        self.set_preset_enabled_safely(preset, false)
    }

    /// Checks which compatible presets are currently enabled in the connected vehicle.
    pub fn check_presets_status(&mut self) -> Result<Vec<PresetStatus>> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };
        let mut ecu_traces: HashMap<String, FswPswFile> = HashMap::new();
        let mut statuses = Vec::new();

        for preset in builtin_presets() {
            if !preset.supports_chassis(&vehicle.chassis) {
                continue;
            }
            let Some(ecu) = vehicle
                .ecus
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(&preset.target_ecu))
            else {
                continue;
            };
            if !ecu_traces.contains_key(&ecu.name) {
                let trace = self.read_ecu_coding(ecu)?;
                ecu_traces.insert(ecu.name.clone(), trace);
            }
            let enabled = ecu_traces
                .get(&ecu.name)
                .map(|t| t.is_preset_enabled(&preset.changes))
                .unwrap_or(false);
            statuses.push(PresetStatus {
                id: preset.id,
                enabled,
            });
        }

        Ok(statuses)
    }

    fn set_preset_enabled_safely(
        &mut self,
        preset: &CodingPreset,
        enable: bool,
    ) -> Result<CodingReport> {
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

        self.apply_changes_mode_safely(&vehicle.vin, &ecu, &preset.changes, enable)
    }

    /// Safely applies arbitrary `PresetChange` edits to an ECU with backup + verify.
    pub fn apply_changes_safely(
        &mut self,
        vin: &str,
        ecu: &EcuInfo,
        changes: &[PresetChange],
    ) -> Result<CodingReport> {
        self.apply_changes_mode_safely(vin, ecu, changes, true)
    }

    /// Verifies battery voltage, link stability, engine-off (RPM == 0), and standstill before any ECU write.
    fn verify_safe_write_conditions(&mut self) -> Result<()> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };
        if !vehicle.battery_voltage.is_finite() || vehicle.battery_voltage < MIN_SAFE_CODING_VOLTAGE {
            return Err(HandosError::LowVoltage {
                voltage: vehicle.battery_voltage,
                minimum: MIN_SAFE_CODING_VOLTAGE,
            });
        }

        match self.read_live_telemetry() {
            Ok(sample) => {
                if !sample.battery_voltage.is_finite()
                    || sample.battery_voltage < MIN_SAFE_CODING_VOLTAGE
                {
                    return Err(HandosError::LowVoltage {
                        voltage: sample.battery_voltage,
                        minimum: MIN_SAFE_CODING_VOLTAGE,
                    });
                }
                if sample.rpm > 0 {
                    return Err(HandosError::UnsafeVehicleState {
                        reason: format!(
                            "engine is running ({} RPM); turn engine off with ignition ON before coding",
                            sample.rpm
                        ),
                    });
                }
                if sample.vehicle_speed_kmh > 0.5 {
                    return Err(HandosError::UnsafeVehicleState {
                        reason: format!(
                            "vehicle is moving ({:.1} km/h); stop vehicle before coding",
                            sample.vehicle_speed_kmh
                        ),
                    });
                }
            }
            Err(HandosError::Io(e)) => return Err(HandosError::Io(e)),
            Err(HandosError::Connection { address, reason }) => {
                return Err(HandosError::Connection { address, reason });
            }
            Err(_) => {
                // Gateway does not support live telemetry (SID 0x22); initial scan voltage check passed.
            }
        }
        Ok(())
    }

    fn apply_changes_mode_safely(
        &mut self,
        vin: &str,
        ecu: &EcuInfo,
        changes: &[PresetChange],
        enable: bool,
    ) -> Result<CodingReport> {
        if ecu.coding_index.trim().is_empty() || ecu.coding_index.eq_ignore_ascii_case("UNKNOWN") {
            return Err(HandosError::Protocol(format!(
                "refusing to code ECU '{}' with missing/unknown coding index",
                ecu.name
            )));
        }

        self.verify_safe_write_conditions()?;

        let original = self.read_ecu_coding(ecu)?;
        if original.is_empty() {
            return Err(HandosError::Protocol(format!(
                "ECU '{}' returned empty coding trace; aborting write",
                ecu.name
            )));
        }
        let backup_entry = self.backups.save_backup(vin, &ecu.name, &original)?;
        let verified_backup = self.backups.load_backup(&backup_entry.path)?;
        if verified_backup.len() != original.len() {
            return Err(HandosError::Backup(format!(
                "pre-write backup verification failed for '{}'; aborting write",
                backup_entry.path
            )));
        }

        let mut modified = original.clone();
        let diffs = if enable {
            modified.apply_preset_changes(changes)?
        } else {
            modified.revert_preset_changes(changes)?
        };
        if diffs.is_empty() {
            return Ok(CodingReport {
                ecu: ecu.name.clone(),
                backup_path: backup_entry.path,
                applied_diffs: diffs,
                verified: true,
            });
        }

        if modified.len() != original.len() {
            return Err(HandosError::Protocol(format!(
                "pre-write integrity check failed for {}: parameter count changed ({} -> {})",
                ecu.name,
                original.len(),
                modified.len()
            )));
        }
        let roundtrip = FswPswFile::parse(&modified.to_trc_string())?;
        if roundtrip.len() != modified.len() {
            return Err(HandosError::Protocol(format!(
                "pre-write TRC serialization check failed for {}; aborting write",
                ecu.name
            )));
        }

        if let Err(write_err) = self.write_ecu_coding(ecu, &modified) {
            let _ = self.write_ecu_coding(ecu, &original);
            return Err(write_err);
        }

        // Re-read and verify every changed parameter AND ensure no unrelated parameters were corrupted.
        let verify_read = match self.read_ecu_coding(ecu) {
            Ok(v) => v,
            Err(read_err) => {
                let _ = self.write_ecu_coding(ecu, &original);
                return Err(read_err);
            }
        };
        for diff in &diffs {
            let actual = verify_read
                .get_single(&diff.parameter)
                .unwrap_or("<missing>");
            if actual != diff.new_value {
                let rollback_ok = self.write_ecu_coding(ecu, &original).is_ok();
                return Err(HandosError::VerificationFailed {
                    ecu: ecu.name.clone(),
                    parameter: diff.parameter.clone(),
                    expected: diff.new_value.clone(),
                    actual: actual.to_string(),
                    rollback_ok,
                });
            }
        }
        for (param, orig_vals) in original.entries() {
            if diffs.iter().any(|d| d.parameter == *param) {
                continue;
            }
            let actual_vals = verify_read.get(param);
            if actual_vals != Some(orig_vals.as_slice()) {
                let rollback_ok = self.write_ecu_coding(ecu, &original).is_ok();
                return Err(HandosError::VerificationFailed {
                    ecu: ecu.name.clone(),
                    parameter: param.clone(),
                    expected: orig_vals.join(","),
                    actual: actual_vals
                        .map(|v| v.join(","))
                        .unwrap_or_else(|| "<missing>".to_string()),
                    rollback_ok,
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

    /// Restores an ECU from a previously saved `.trc` backup file, enforcing voltage & verification.
    pub fn restore_backup(&mut self, ecu: &EcuInfo, backup_path: &str) -> Result<Vec<CodingDiff>> {
        self.verify_safe_write_conditions()?;
        let backup_data = self.backups.load_backup(backup_path)?;
        if backup_data.is_empty() {
            return Err(HandosError::Backup(
                "refusing to restore empty backup file".to_string(),
            ));
        }
        let current = self.read_ecu_coding(ecu)?;
        let has_overlap = backup_data
            .entries()
            .keys()
            .any(|k| current.get(k).is_some());
        if !has_overlap {
            return Err(HandosError::Backup(format!(
                "backup '{}' has no matching parameters for ECU '{}'; refusing cross-ECU restore",
                backup_path, ecu.name
            )));
        }

        let diffs = current.diff_against(&backup_data);
        if let Err(write_err) = self.write_ecu_coding(ecu, &backup_data) {
            let _ = self.write_ecu_coding(ecu, &current);
            return Err(write_err);
        }

        let verify = match self.read_ecu_coding(ecu) {
            Ok(v) => v,
            Err(read_err) => {
                let _ = self.write_ecu_coding(ecu, &current);
                return Err(read_err);
            }
        };
        for diff in &diffs {
            let actual = verify.get_single(&diff.parameter).unwrap_or("<missing>");
            if actual != diff.new_value {
                let rollback_ok = self.write_ecu_coding(ecu, &current).is_ok();
                return Err(HandosError::VerificationFailed {
                    ecu: ecu.name.clone(),
                    parameter: diff.parameter.clone(),
                    expected: diff.new_value.clone(),
                    actual: actual.to_string(),
                    rollback_ok,
                });
            }
        }
        Ok(diffs)
    }

    /// Reads fault codes (DTCs / error memory) from a specific ECU and enriches
    /// descriptions using the offline BMW/OBD DTC catalog.
    pub fn read_dtcs(&mut self, ecu: &EcuInfo) -> Result<Vec<Dtc>> {
        let req = DiagnosticFrame::diagnostic(ecu.address, vec![sid::READ_DTC, 0x00]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive(&ecu.name, sid::READ_DTC)?;
        let mut dtcs: Vec<Dtc> = serde_json::from_slice(payload)?;
        for dtc in &mut dtcs {
            dtc.description = resolve_dtc_description(&dtc.code, &dtc.description);
        }
        Ok(dtcs)
    }

    /// Reads live engine/vehicle telemetry (boost, oil temp, coolant temp, RPM, voltage).
    pub fn read_live_telemetry(&mut self) -> Result<TelemetrySample> {
        let req = DiagnosticFrame::adapter_control(vec![sid::READ_TELEMETRY]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive("GATEWAY", sid::READ_TELEMETRY)?;
        let sample: TelemetrySample = serde_json::from_slice(payload)?;
        Ok(sample)
    }

    /// Executes a BMW maintenance/service job (e.g. battery registration, CBS reset).
    pub fn run_service_job(&mut self, job: ServiceJob) -> Result<ServiceReport> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };
        if vehicle.battery_voltage < MIN_SAFE_CODING_VOLTAGE {
            return Err(HandosError::LowVoltage {
                voltage: vehicle.battery_voltage,
                minimum: MIN_SAFE_CODING_VOLTAGE,
            });
        }
        let req = DiagnosticFrame::adapter_control(vec![sid::SERVICE_ROUTINE, job.routine_id()]);
        let resp = self.transport.exchange(&req)?;
        let payload = resp.check_positive("GATEWAY", sid::SERVICE_ROUTINE)?;
        let report: ServiceReport = serde_json::from_slice(payload)?;
        Ok(report)
    }

    /// Reads all `FSW_PSW` parameters from an ECU by name for the Expert browser.
    pub fn read_ecu_params(&mut self, ecu_name: &str) -> Result<Vec<(String, String)>> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };
        let ecu = vehicle
            .ecus
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(ecu_name))
            .cloned()
            .ok_or_else(|| HandosError::Protocol(format!("ECU '{ecu_name}' not found")))?;
        let trace = self.read_ecu_coding(&ecu)?;
        Ok(trace
            .parameters()
            .iter()
            .map(|p| (p.clone(), trace.get_single(p).unwrap_or("").to_string()))
            .collect())
    }

    /// Safely updates a single `FSW_PSW` parameter on an ECU (with backup + verification).
    pub fn apply_single_param_safely(
        &mut self,
        ecu_name: &str,
        parameter: &str,
        value: &str,
    ) -> Result<CodingReport> {
        let vehicle = match &self.cached_vehicle {
            Some(v) => v.clone(),
            None => self.scan_vehicle()?,
        };
        if vehicle.battery_voltage < MIN_SAFE_CODING_VOLTAGE {
            return Err(HandosError::LowVoltage {
                voltage: vehicle.battery_voltage,
                minimum: MIN_SAFE_CODING_VOLTAGE,
            });
        }
        let ecu = vehicle
            .ecus
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(ecu_name))
            .cloned()
            .ok_or_else(|| HandosError::Protocol(format!("ECU '{ecu_name}' not found")))?;
        let change = PresetChange {
            parameter: parameter.to_string(),
            value: value.to_string(),
            off_value: "nicht_aktiv".to_string(),
            aliases: vec![],
        };
        self.apply_changes_safely(&vehicle.vin, &ecu, &[change])
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

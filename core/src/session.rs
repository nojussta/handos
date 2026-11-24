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

    fn apply_changes_mode_safely(
        &mut self,
        vin: &str,
        ecu: &EcuInfo,
        changes: &[PresetChange],
        enable: bool,
    ) -> Result<CodingReport> {
        let original = self.read_ecu_coding(ecu)?;
        let backup_entry = self.backups.save_backup(vin, &ecu.name, &original)?;

        let mut modified = original;
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

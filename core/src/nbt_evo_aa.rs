use crate::error::{HandosError, Result};
use crate::presets::builtin_presets;
use crate::session::HandosSession;
use crate::transport::Transport;
use crate::types::{CodingReport, EcuInfo, VehicleInfo};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// Minimum battery voltage required before modifying head-unit coding or files.
pub const NBT_EVO_MIN_VOLTAGE: f32 = 12.0;

/// Default NBT EVO head-unit ENET link-local IP & SSH port.
pub const DEFAULT_NBT_EVO_HOST: &str = "169.254.199.99:22";

/// Result of verifying whether an NBT EVO head unit meets all requirements
/// before any coding or file push is attempted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NbtEvoPreflight {
    pub vin: String,
    pub head_unit_ecu: EcuInfo,
    pub battery_voltage: f32,
    pub is_f_series: bool,
    pub is_id5_or_id6: bool,
    pub ssh_reachable: bool,
    pub bundle_present: bool,
    pub blocking_reasons: Vec<String>,
}

impl NbtEvoPreflight {
    pub fn is_ready(&self) -> bool {
        self.blocking_reasons.is_empty()
    }
}

/// Abstract SSH/SFTP deployer so tests and future SSH backends can plug in cleanly.
pub trait HeadUnitSshClient {
    fn is_port_open(&self, address: &str) -> bool;
    fn backup_remote_file(&mut self, remote_path: &str, local_backup_dir: &Path) -> Result<()>;
    fn upload_file(&mut self, local_path: &Path, remote_path: &str) -> Result<()>;
}

/// Default TCP probe for checking whether SSH (port 22) is unlocked on NBT EVO.
pub struct TcpSshProbe {
    pub timeout: Duration,
}

impl Default for TcpSshProbe {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(2),
        }
    }
}

impl HeadUnitSshClient for TcpSshProbe {
    fn is_port_open(&self, address: &str) -> bool {
        let Ok(mut addrs) = address.to_socket_addrs() else {
            return false;
        };
        let Some(sock): Option<SocketAddr> = addrs.next() else {
            return false;
        };
        TcpStream::connect_timeout(&sock, self.timeout).is_ok()
    }

    fn backup_remote_file(&mut self, _remote_path: &str, _local_backup_dir: &Path) -> Result<()> {
        Err(HandosError::Protocol(
            "SFTP backend not yet linked: unlock SSH and provide SFTP transport".to_string(),
        ))
    }

    fn upload_file(&mut self, _local_path: &Path, _remote_path: &str) -> Result<()> {
        Err(HandosError::Protocol(
            "SFTP backend not yet linked: unlock SSH and provide SFTP transport".to_string(),
        ))
    }
}

/// Summary of a safe NBT EVO Android Auto installation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NbtEvoInstallReport {
    pub preflight: NbtEvoPreflight,
    pub coding_report: CodingReport,
    pub deployed_files: Vec<String>,
}

/// Evaluates all safety requirements before touching `HU_NBT` / `HU_NBT2`.
pub fn run_nbt_evo_preflight(
    vehicle: &VehicleInfo,
    ssh_client: &dyn HeadUnitSshClient,
    ssh_address: &str,
    bundle_dir: Option<&Path>,
) -> NbtEvoPreflight {
    let mut blocking = Vec::new();
    let is_f_series = vehicle.chassis.platform_family() == "Fxx";
    if !is_f_series {
        blocking.push(format!(
            "chassis {} is not F-Series (Fxx required for NBT EVO)",
            vehicle.chassis
        ));
    }

    if vehicle.battery_voltage < NBT_EVO_MIN_VOLTAGE {
        blocking.push(format!(
            "battery voltage {:.2}V is below safe head-unit threshold ({:.2}V)",
            vehicle.battery_voltage, NBT_EVO_MIN_VOLTAGE
        ));
    }

    let hu = vehicle
        .ecus
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("HU_NBT") || e.name.eq_ignore_ascii_case("HU_NBT2"))
        .cloned();

    let (head_unit_ecu, is_id5_or_id6) = match hu {
        Some(ecu) => {
            let idx = ecu.coding_index.to_uppercase();
            let id5_6 = idx.contains("NBT2")
                || idx.contains("EVO")
                || idx.contains("ID5")
                || idx.contains("ID6");
            if !id5_6 {
                blocking.push(format!(
                    "head unit coding index '{}' is not confirmed as NBT EVO (ID5/ID6)",
                    ecu.coding_index
                ));
            }
            (ecu, id5_6)
        }
        None => {
            blocking.push("no HU_NBT / HU_NBT2 head unit ECU detected on vehicle".to_string());
            (
                EcuInfo {
                    address: 0x63,
                    name: "HU_NBT2".to_string(),
                    description: "Not detected".to_string(),
                    coding_index: "UNKNOWN".to_string(),
                },
                false,
            )
        }
    };

    let ssh_reachable = ssh_client.is_port_open(ssh_address);
    if !ssh_reachable {
        blocking.push(format!(
            "SSH is locked/unreachable at {ssh_address}; unlock SSH via firmware-specific method first"
        ));
    }

    let bundle_present = bundle_dir
        .map(|d| d.exists() && d.join("manifest.json").exists())
        .unwrap_or(false);
    if !bundle_present {
        blocking.push(
            "verified NBT EVO Android Auto QNX bundle (manifest.json + binaries) is missing"
                .to_string(),
        );
    }

    NbtEvoPreflight {
        vin: vehicle.vin.clone(),
        head_unit_ecu,
        battery_voltage: vehicle.battery_voltage,
        is_f_series,
        is_id5_or_id6,
        ssh_reachable,
        bundle_present,
        blocking_reasons: blocking,
    }
}

/// Safely orchestrates NBT EVO Android Auto installation:
/// 1. Runs strict pre-flight checks (blocks immediately if any requirement fails).
/// 2. Backs up `HU_NBT` / `HU_NBT2` coding data and applies `fxx_nbt_evo_aa_prep`.
/// 3. Backs up remote head-unit files and uploads verified bundle entries.
pub fn install_nbt_evo_android_auto<T: Transport>(
    session: &mut HandosSession<T>,
    ssh_client: &mut dyn HeadUnitSshClient,
    ssh_address: &str,
    bundle_dir: &Path,
    remote_backup_dir: &Path,
) -> Result<NbtEvoInstallReport> {
    let vehicle = session.scan_vehicle()?;
    let preflight = run_nbt_evo_preflight(&vehicle, ssh_client, ssh_address, Some(bundle_dir));
    if !preflight.is_ready() {
        return Err(HandosError::Protocol(format!(
            "NBT EVO pre-flight failed: {}",
            preflight.blocking_reasons.join("; ")
        )));
    }

    let prep_preset = builtin_presets()
        .into_iter()
        .find(|p| p.id == "fxx_nbt_evo_aa_prep")
        .ok_or_else(|| HandosError::Protocol("missing fxx_nbt_evo_aa_prep preset".to_string()))?;

    let coding_report = session.apply_preset_safely(&prep_preset)?;

    // Backup existing head-unit config before uploading anything
    std::fs::create_dir_all(remote_backup_dir)?;
    ssh_client.backup_remote_file("/var/opt/sys/persistence/aa_config.json", remote_backup_dir)?;

    let manifest_raw = std::fs::read_to_string(bundle_dir.join("manifest.json"))?;
    let files: Vec<(String, String)> = serde_json::from_str(&manifest_raw)?;
    let mut deployed_files = Vec::with_capacity(files.len());
    for (local_rel, remote_path) in files {
        let local_path = bundle_dir.join(&local_rel);
        if !local_path.exists() {
            return Err(HandosError::Protocol(format!(
                "bundle file '{}' missing",
                local_path.display()
            )));
        }
        ssh_client.upload_file(&local_path, &remote_path)?;
        deployed_files.push(remote_path);
    }

    Ok(NbtEvoInstallReport {
        preflight,
        coding_report,
        deployed_files,
    })
}

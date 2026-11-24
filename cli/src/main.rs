use handos_core::{
    builtin_presets, BackupManager, HandosSession, MhdWifiTransport, DEFAULT_MHD_WIFI_HOST,
    DEFAULT_MHD_WIFI_PORT,
};
use std::env;
use std::process;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut host = format!("{DEFAULT_MHD_WIFI_HOST}:{DEFAULT_MHD_WIFI_PORT}");
    let mut backup_dir = "backups".to_string();
    let mut positional = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" if i + 1 < args.len() => {
                host = args[i + 1].clone();
                i += 2;
            }
            "--backups" if i + 1 < args.len() => {
                backup_dir = args[i + 1].clone();
                i += 2;
            }
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }

    if positional.is_empty() {
        print_usage();
        return;
    }

    if positional[0] == "presets" {
        for p in builtin_presets() {
            println!(
                "{} [{} -> {}]: {}",
                p.id,
                p.platform_families.join(","),
                p.target_ecu,
                p.title
            );
        }
        return;
    }

    let transport = MhdWifiTransport::connect(&host, Duration::from_secs(5)).unwrap_or_else(|e| {
        eprintln!("Connection error: {e}");
        process::exit(1);
    });
    let backups = BackupManager::new(&backup_dir).unwrap_or_else(|e| {
        eprintln!("Backup dir error: {e}");
        process::exit(1);
    });
    let mut session = HandosSession::new(transport, backups);

    match positional[0].as_str() {
        "scan" => {
            let info = session.scan_vehicle().unwrap_or_else(fail);
            println!(
                "VIN: {} | Chassis: {} | Battery: {:.2}V",
                info.vin, info.chassis, info.battery_voltage
            );
            for ecu in &info.ecus {
                println!(
                    "  [0x{:02X}] {:<6} ({}) - {}",
                    ecu.address, ecu.name, ecu.coding_index, ecu.description
                );
            }
        }
        "apply-preset" => {
            let id = positional.get(1).unwrap_or_else(|| {
                eprintln!("Missing preset id. Run `handos-cli presets` to list.");
                process::exit(1);
            });
            let preset = builtin_presets()
                .into_iter()
                .find(|p| &p.id == id)
                .unwrap_or_else(|| {
                    eprintln!("Unknown preset '{id}'");
                    process::exit(1);
                });
            let report = session.apply_preset_safely(&preset).unwrap_or_else(fail);
            println!(
                "Coded {} (verified={}) | Backup saved: {}",
                report.ecu, report.verified, report.backup_path
            );
            for diff in &report.applied_diffs {
                println!(
                    "  {}: {} -> {}",
                    diff.parameter, diff.old_value, diff.new_value
                );
            }
        }
        "dtc-read" => {
            let info = session.scan_vehicle().unwrap_or_else(fail);
            for ecu in &info.ecus {
                let dtcs = session.read_dtcs(ecu).unwrap_or_else(fail);
                if dtcs.is_empty() {
                    println!("{}: No fault codes", ecu.name);
                } else {
                    for d in dtcs {
                        println!(
                            "{} [{}] (active={}): {}",
                            d.ecu, d.code, d.active, d.description
                        );
                    }
                }
            }
        }
        "dtc-clear" => {
            let info = session.scan_vehicle().unwrap_or_else(fail);
            for ecu in &info.ecus {
                session.clear_dtcs(ecu).unwrap_or_else(fail);
                println!("Cleared fault memory on {}", ecu.name);
            }
        }
        _ => print_usage(),
    }
}

fn fail<T, E: std::fmt::Display>(err: E) -> T {
    eprintln!("Error: {err}");
    process::exit(1);
}

fn print_usage() {
    println!(
        "handos-cli [--host 192.168.4.1:6801] [--backups backups] <command>
Commands:
  presets              List built-in coding presets
  scan                 Connect, read VIN, voltage, and reachable ECUs
  apply-preset <id>    Backup, apply preset, and verify ECU coding
  dtc-read             Read fault codes from all ECUs
  dtc-clear            Clear fault codes on all ECUs"
    );
}

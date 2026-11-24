use handos_core::{
    builtin_presets, BackupManager, Chassis, HandosError, HandosSession, MhdWifiTransport,
};
use handos_sim::{spawn_simulator, SimulatedCar};
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn e90_and_e87_full_coding_backup_restore_and_dtc_flow() {
    for (car, expected_chassis) in [
        (SimulatedCar::e90(), Chassis::E90),
        (SimulatedCar::e87(), Chassis::E87),
    ] {
        let addr = spawn_simulator("127.0.0.1:0", car).unwrap();
        let transport = MhdWifiTransport::connect(&addr, Duration::from_secs(3)).unwrap();
        let backup_dir = TempDir::new().unwrap();
        let backups = BackupManager::new(backup_dir.path()).unwrap();
        let mut session = HandosSession::new(transport, backups);

        let vehicle = session.scan_vehicle().unwrap();
        assert_eq!(vehicle.chassis, expected_chassis);
        assert_eq!(vehicle.ecus.len(), 3);

        // Apply E8x/E9x USDM corners + Double Hazard + Start/Stop Memory
        let presets = builtin_presets();
        for id in [
            "e8x_e9x_usdm_corners",
            "e8x_e9x_double_hazard",
            "e8x_e9x_start_stop_memory",
        ] {
            let preset = presets.iter().find(|p| p.id == id).unwrap();
            let report = session.apply_preset_safely(preset).unwrap();
            assert!(report.verified);
            assert!(!report.applied_diffs.is_empty());
        }

        // Reject E46 preset on E8x/E9x vehicle
        let e46_preset = presets.iter().find(|p| p.id == "e46_usdm_corners").unwrap();
        let err = session.apply_preset_safely(e46_preset).unwrap_err();
        assert!(matches!(err, HandosError::ChassisMismatch { .. }));

        // Verify backup restore reverts FRM back to original
        let frm = vehicle.ecus.iter().find(|e| e.name == "FRM").unwrap();
        let backups_list = session.backups().list_backups(&vehicle.vin).unwrap();
        let oldest_frm_backup = backups_list.iter().rfind(|b| b.ecu == "FRM").unwrap();
        let restore_diffs = session
            .restore_backup(frm, &oldest_frm_backup.path)
            .unwrap();
        assert!(!restore_diffs.is_empty());
        let frm_after = session.read_ecu_coding(frm).unwrap();
        assert_eq!(frm_after.get_single("SIDEMARKER_US"), Some("nicht_aktiv"));

        // Read & Clear DTCs
        let dtcs = session.read_dtcs(frm).unwrap();
        assert_eq!(dtcs.len(), 1);
        session.clear_dtcs(frm).unwrap();
        let dtcs_after = session.read_dtcs(frm).unwrap();
        assert!(dtcs_after.is_empty());
    }
}

#[test]
fn e46_low_voltage_blocks_coding() {
    let mut car = SimulatedCar::e46();
    car.vehicle.battery_voltage = 11.1; // Below 11.5V safety threshold
    let addr = spawn_simulator("127.0.0.1:0", car).unwrap();

    let transport = MhdWifiTransport::connect(&addr, Duration::from_secs(3)).unwrap();
    let backup_dir = TempDir::new().unwrap();
    let backups = BackupManager::new(backup_dir.path()).unwrap();
    let mut session = HandosSession::new(transport, backups);

    let preset = builtin_presets()
        .into_iter()
        .find(|p| p.id == "e46_usdm_corners")
        .unwrap();
    let err = session.apply_preset_safely(&preset).unwrap_err();
    assert!(matches!(err, HandosError::LowVoltage { .. }));
}

#[test]
fn f30_coding_flow_succeeds() {
    let addr = spawn_simulator("127.0.0.1:0", SimulatedCar::f30()).unwrap();
    let transport = MhdWifiTransport::connect(&addr, Duration::from_secs(3)).unwrap();
    let backup_dir = TempDir::new().unwrap();
    let backups = BackupManager::new(backup_dir.path()).unwrap();
    let mut session = HandosSession::new(transport, backups);

    let vehicle = session.scan_vehicle().unwrap();
    assert_eq!(vehicle.chassis, Chassis::F30);

    let presets = builtin_presets();
    for id in [
        "fxx_start_stop_memory",
        "fxx_digital_speedo",
        "fxx_video_in_motion",
        "fxx_nbt_evo_aa_prep",
        "fxx_nbt_evo_carplay_fullscreen",
    ] {
        let preset = presets.iter().find(|p| p.id == id).unwrap();
        let report = session.apply_preset_safely(preset).unwrap();
        assert!(report.verified);
    }
}

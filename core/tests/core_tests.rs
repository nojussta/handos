use handos_core::fsw_psw::{parse_sp_daten_defs, FswPswFile};
use handos_core::{
    builtin_ecu_schemas, builtin_presets, parse_ncsdummy_xml, resolve_dtc_description,
    run_nbt_evo_preflight, BackupManager, Chassis, DataLogger, DiagnosticFrame, Ds2Packet, EcuInfo,
    HeadUnitSshClient, KdcanProtocolMode, KdcanSerialTransport, KwpPacket, NettodatImage,
    PresetChange, SchemaRegistry, TelemetrySample, Transport, VehicleInfo,
};
use tempfile::TempDir;

#[test]
fn parses_and_roundtrips_fsw_psw_trc() {
    let raw = "\
// BMW FSW_PSW.TRC
SIDEMARKER_US
\tnicht_aktiv
BLINKIMPULS_WARNBLK
\twert_01
";
    let mut parsed = FswPswFile::parse(raw).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed.get_single("SIDEMARKER_US"), Some("nicht_aktiv"));
    assert_eq!(parsed.get_single("BLINKIMPULS_WARNBLK"), Some("wert_01"));

    let diffs = parsed
        .apply_preset_changes(&[PresetChange {
            parameter: "SIDEMARKER_US".to_string(),
            value: "aktiv".to_string(),
            off_value: "nicht_aktiv".to_string(),
            aliases: vec![],
        }])
        .unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].old_value, "nicht_aktiv");
    assert_eq!(diffs[0].new_value, "aktiv");

    let serialized = parsed.to_trc_string();
    let reparsed = FswPswFile::parse(&serialized).unwrap();
    assert_eq!(reparsed.get_single("SIDEMARKER_US"), Some("aktiv"));
}

#[test]
fn resolves_parameter_aliases_across_coding_indices() {
    let mut parsed = FswPswFile::parse("DOPPELBLINKIMPULS_WARNBLK\n\tnicht_aktiv\n").unwrap();
    let diffs = parsed
        .apply_preset_changes(&[PresetChange {
            parameter: "BLINKIMPULS_WARNBLK".to_string(),
            value: "aktiv".to_string(),
            off_value: "nicht_aktiv".to_string(),
            aliases: vec!["DOPPELBLINKIMPULS_WARNBLK".to_string()],
        }])
        .unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].parameter, "DOPPELBLINKIMPULS_WARNBLK");
    assert_eq!(
        parsed.get_single("DOPPELBLINKIMPULS_WARNBLK"),
        Some("aktiv")
    );
}

#[test]
fn saves_lists_and_loads_backups() {
    let dir = TempDir::new().unwrap();
    let mgr = BackupManager::new(dir.path()).unwrap();
    let trc = FswPswFile::parse("MSA_MEMORY\n\tnicht_aktiv\n").unwrap();

    let saved = mgr.save_backup("WBAVA31080NL54321", "IHKA", &trc).unwrap();
    let list = mgr.list_backups("WBAVA31080NL54321").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].ecu, "IHKA");

    let loaded = mgr.load_backup(&saved.path).unwrap();
    assert_eq!(loaded, trc);
}

#[test]
fn builtin_presets_support_e46_e87_e90_and_f_series() {
    let presets = builtin_presets();
    let e46 = presets.iter().find(|p| p.id == "e46_usdm_corners").unwrap();
    let e8x_e9x = presets
        .iter()
        .find(|p| p.id == "e8x_e9x_usdm_corners")
        .unwrap();
    let fxx = presets
        .iter()
        .find(|p| p.id == "fxx_start_stop_memory")
        .unwrap();

    assert!(e46.supports_chassis(&Chassis::E46));
    assert!(!e46.supports_chassis(&Chassis::E87));

    assert!(e8x_e9x.supports_chassis(&Chassis::E87));
    assert!(e8x_e9x.supports_chassis(&Chassis::E90));
    assert!(e8x_e9x.supports_chassis(&Chassis::E84));

    assert!(fxx.supports_chassis(&Chassis::F10));
    assert!(fxx.supports_chassis(&Chassis::F20));
    assert!(fxx.supports_chassis(&Chassis::F30));
    assert!(!fxx.supports_chassis(&Chassis::E90));
}

#[test]
fn parses_sp_daten_definitions() {
    let sample = include_str!("../../presets/sp_daten_sample.txt");
    let defs = parse_sp_daten_defs(sample);
    assert!(defs.len() >= 5);
    assert_eq!(defs[0].name, "SIDEMARKER_US");
    assert_eq!(defs[0].possible_values, vec!["nicht_aktiv", "aktiv"]);
}

#[test]
fn nettodat_bitmask_codec_preserves_unrelated_bits() {
    let schemas = builtin_ecu_schemas();
    let frm_schema = schemas.iter().find(|s| s.ecu_name == "FRM").unwrap();

    // Byte 0 has unrelated upper bits (0xF0), byte 1 has wert_01 (0x01) + upper 0xA0, byte 2 has 0x00
    let mut image = NettodatImage::from_single_block(0x3000, vec![0xF0, 0xA1, 0x00]);
    let mut fsw = frm_schema.decode(&image).unwrap();
    assert_eq!(fsw.get_single("SIDEMARKER_US"), Some("nicht_aktiv"));
    assert_eq!(fsw.get_single("PWM_SL_V_BEI_AKTIVEN_AL"), Some("wert_01"));

    fsw.set("SIDEMARKER_US", "aktiv");
    fsw.set("PWM_SL_V_BEI_AKTIVEN_AL", "wert_04");
    let diffs = frm_schema.encode_onto(&mut image, &fsw).unwrap();
    assert_eq!(diffs.len(), 2);

    // Verify unrelated upper bits 0xF0 and 0xA0 were preserved untouched
    assert_eq!(image.blocks[&0x3000], vec![0xF1, 0xA4, 0x00]);

    let trc_dump = image.to_nettodat_trc();
    let reparsed = NettodatImage::parse_nettodat_trc(&trc_dump).unwrap();
    assert_eq!(reparsed, image);
}

#[test]
fn kwp2000_and_ds2_packets_encode_decode_with_checksum() {
    let kwp = KwpPacket::new(0x72, vec![0x1A, 0x90]);
    let raw_kwp = kwp.encode().unwrap();
    let decoded_kwp = KwpPacket::decode(&raw_kwp).unwrap();
    assert_eq!(decoded_kwp, kwp);

    let ds2 = Ds2Packet::new(0xD0, vec![0x00, 0x01]);
    let raw_ds2 = ds2.encode().unwrap();
    let decoded_ds2 = Ds2Packet::decode(&raw_ds2).unwrap();
    assert_eq!(decoded_ds2, ds2);
}

#[test]
fn schema_registry_dynamically_imports_xml_and_resolves_by_coding_index() {
    let dir = TempDir::new().unwrap();
    let xml = r#"
<module ecu="FRM" coding_index="FRM3_E89.C32">
  <function name="SIDEMARKER_US" block="0x3000" offset="0" mask="0x01">
    <param name="nicht_aktiv" value="0x00"/>
    <param name="aktiv" value="0x01"/>
  </function>
</module>
"#;
    std::fs::write(dir.path().join("frm3.xml"), xml).unwrap();
    let registry = SchemaRegistry::load(dir.path()).unwrap();

    let ecu = EcuInfo {
        address: 0x72,
        name: "FRM".to_string(),
        description: "Footwell Module 3".to_string(),
        coding_index: "FRM3_E89.C32".to_string(),
    };
    let resolved = registry.resolve_for_ecu(&ecu).unwrap();
    assert_eq!(resolved.coding_index_prefix, "FRM3_E89.C32");

    let parsed = parse_ncsdummy_xml(xml).unwrap();
    assert_eq!(parsed.params.len(), 1);
    assert_eq!(parsed.params[0].parameter, "SIDEMARKER_US");
}

struct MockSsh {
    open: bool,
}

impl HeadUnitSshClient for MockSsh {
    fn is_port_open(&self, _address: &str) -> bool {
        self.open
    }
    fn backup_remote_file(
        &mut self,
        _remote_path: &str,
        _local_backup_dir: &std::path::Path,
    ) -> handos_core::Result<()> {
        Ok(())
    }
    fn upload_file(
        &mut self,
        _local_path: &std::path::Path,
        _remote_path: &str,
    ) -> handos_core::Result<()> {
        Ok(())
    }
}

#[test]
fn nbt_evo_preflight_blocks_unsafe_or_incomplete_targets() {
    let dir = TempDir::new().unwrap();
    let vehicle = VehicleInfo {
        vin: "WBA3B1C50DF123456".to_string(),
        chassis: Chassis::E90,
        battery_voltage: 11.4,
        ecus: vec![EcuInfo {
            address: 0x63,
            name: "HU_NBT".to_string(),
            description: "NBT".to_string(),
            coding_index: "HU_NBT.C01".to_string(),
        }],
    };
    let report = run_nbt_evo_preflight(
        &vehicle,
        &MockSsh { open: false },
        "169.254.199.99:22",
        Some(dir.path()),
    );
    assert!(!report.is_ready());
    assert_eq!(report.blocking_reasons.len(), 5);
}

#[test]
fn nbt_evo_preflight_passes_when_all_requirements_met() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("manifest.json"), "[]").unwrap();
    let vehicle = VehicleInfo {
        vin: "WBA3B1C50DF123456".to_string(),
        chassis: Chassis::F30,
        battery_voltage: 12.6,
        ecus: vec![EcuInfo {
            address: 0x63,
            name: "HU_NBT".to_string(),
            description: "NBT EVO ID6".to_string(),
            coding_index: "HU_NBT2_EVO_ID6".to_string(),
        }],
    };
    let report = run_nbt_evo_preflight(
        &vehicle,
        &MockSsh { open: true },
        "169.254.199.99:22",
        Some(dir.path()),
    );
    assert!(report.is_ready());
}

struct LoopbackSerial(std::collections::VecDeque<u8>);

impl std::io::Read for LoopbackSerial {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for LoopbackSerial {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn kdcan_serial_transport_exchanges_frames() {
    let mut transport = KdcanSerialTransport::new(
        LoopbackSerial(std::collections::VecDeque::new()),
        KdcanProtocolMode::Dcan115200,
        "usb://ftdi",
    );
    assert_eq!(transport.mode().baud_rate(), 115_200);
    let req = DiagnosticFrame::diagnostic(0x72, vec![0x21, 0x01]);
    let resp = transport.exchange(&req).unwrap();
    assert_eq!(resp, req);
}

#[test]
fn dtc_catalog_resolves_offline_bmw_codes() {
    let desc = resolve_dtc_description("30FF", "");
    assert!(desc.contains("underboost"));
    let fallback = resolve_dtc_description("FFFF", "Custom message");
    assert_eq!(fallback, "Custom message");
}

#[test]
fn data_logger_exports_csv_file() {
    let dir = TempDir::new().unwrap();
    let mut logger = DataLogger::new();
    logger.push(TelemetrySample {
        timestamp_ms: 1000,
        battery_voltage: 12.8,
        rpm: 2500,
        coolant_temp_c: 92.0,
        oil_temp_c: 101.0,
        boost_kpa: 165.0,
        intake_temp_c: 34.0,
        vehicle_speed_kmh: 60.0,
    });
    let csv_path = logger.save_csv(dir.path()).unwrap();
    let content = std::fs::read_to_string(csv_path).unwrap();
    assert!(content.contains("boost_kpa"));
    assert!(content.contains("165.0"));

    // Append a second sample to a single session file without duplicating headers
    let session_csv = dir.path().join("session.csv");
    let next_idx = logger.append_csv(&session_csv, 0).unwrap();
    assert_eq!(next_idx, 1);
    logger.push(TelemetrySample {
        timestamp_ms: 2000,
        battery_voltage: 12.7,
        rpm: 3100,
        coolant_temp_c: 93.0,
        oil_temp_c: 102.0,
        boost_kpa: 190.0,
        intake_temp_c: 35.0,
        vehicle_speed_kmh: 80.0,
    });
    let next_idx2 = logger.append_csv(&session_csv, next_idx).unwrap();
    assert_eq!(next_idx2, 2);
    let appended = std::fs::read_to_string(&session_csv).unwrap();
    assert_eq!(appended.lines().count(), 3);
}

#[test]
fn refuses_unknown_parameter_and_empty_backup() {
    let mut trc = FswPswFile::parse("SIDEMARKER_US\n\tnicht_aktiv\n").unwrap();
    let err = trc
        .apply_preset_changes(&[PresetChange {
            parameter: "NON_EXISTENT_PARAM".to_string(),
            value: "aktiv".to_string(),
            off_value: "nicht_aktiv".to_string(),
            aliases: vec![],
        }])
        .unwrap_err();
    assert!(matches!(
        err,
        handos_core::HandosError::UnknownParameter { .. }
    ));

    let dir = TempDir::new().unwrap();
    let mgr = BackupManager::new(dir.path()).unwrap();
    let empty_trc = FswPswFile::new();
    assert!(mgr
        .save_backup("WBAVA31080NL54321", "FRM", &empty_trc)
        .is_err());
}

pub mod backup;
pub mod dtc_catalog;
pub mod error;
pub mod ffi;
pub mod fsw_psw;
pub mod kwp_ds2;
pub mod live_data;
pub mod nbt_evo_aa;
pub mod nettodat;
pub mod presets;
pub mod protocol;
pub mod schema_registry;
pub mod service;
pub mod session;
pub mod transport;
pub mod types;

pub use backup::BackupManager;
pub use dtc_catalog::resolve_dtc_description;
pub use error::{HandosError, Result};
pub use fsw_psw::{parse_sp_daten_defs, FswPswFile, SpDatenParamDef};
pub use kwp_ds2::{kwp_jobs, Ds2Packet, KwpPacket};
pub use live_data::{DataLogger, TelemetrySample};
pub use nbt_evo_aa::{
    install_nbt_evo_android_auto, run_nbt_evo_preflight, scan_nbt_evo_preflight, HeadUnitSshClient,
    NbtEvoFirmware, NbtEvoInstallReport, NbtEvoPreflight, TcpSshProbe, DEFAULT_NBT_EVO_HOST,
    NBT_EVO_MIN_VOLTAGE,
};
pub use nettodat::{CodingParamSchema, EcuCodingSchema, NettodatImage};
pub use presets::{
    builtin_ecu_schemas, builtin_presets, load_presets_from_file, presets_for_chassis,
};
pub use protocol::{DiagnosticFrame, PayloadType, DEFAULT_MHD_WIFI_HOST, DEFAULT_MHD_WIFI_PORT};
pub use schema_registry::{parse_ncsdummy_xml, SchemaRegistry};
pub use service::{ServiceJob, ServiceReport};
pub use session::{HandosSession, MIN_SAFE_CODING_VOLTAGE};
pub use transport::{
    discover_mhd_endpoint, discover_mhd_endpoints, AdapterType, KdcanProtocolMode,
    KdcanSerialTransport, MhdWifiTransport, Transport, TransportMode,
};
pub use types::*;

use crate::error::Result;
use crate::nettodat::EcuCodingSchema;
use crate::types::{Chassis, CodingPreset};
use std::path::Path;

/// Built-in curated presets bundled into `handos-core` for zero-setup use on mobile.
pub fn builtin_presets() -> Vec<CodingPreset> {
    let raw = include_str!("../../../config/presets/builtin_presets.json");
    serde_json::from_str(raw).expect("builtin_presets.json must be valid JSON")
}

/// Built-in binary ECU coding schemas (byte offsets + bitmasks) bundled into `handos-core`.
pub fn builtin_ecu_schemas() -> Vec<EcuCodingSchema> {
    let raw = include_str!("../../../config/presets/ecu_schemas.json");
    serde_json::from_str(raw).expect("ecu_schemas.json must be valid JSON")
}

/// Loads user-imported or custom presets from a JSON file on disk.
pub fn load_presets_from_file(path: impl AsRef<Path>) -> Result<Vec<CodingPreset>> {
    let content = std::fs::read_to_string(path)?;
    let presets: Vec<CodingPreset> = serde_json::from_str(&content)?;
    Ok(presets)
}

/// Filters presets matching a specific BMW chassis (or shared platform family like E8x/E9x).
pub fn presets_for_chassis<'a>(
    presets: &'a [CodingPreset],
    chassis: &Chassis,
) -> Vec<&'a CodingPreset> {
    presets
        .iter()
        .filter(|p| p.supports_chassis(chassis))
        .collect()
}

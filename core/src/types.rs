use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported BMW chassis families (1-series, 3-series, 5-series, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Chassis {
    E46,
    /// 1-Series (E81 / E82 / E87 / E88), shares FRM, CAS, IHKA, KOMBI with E9x.
    E87,
    /// 3-Series (E90 / E91 / E92 / E93), shares FRM, CAS, IHKA, KOMBI with E8x.
    E90,
    E60,
    E84,
    /// F-Series 5-Series (F10 / F11 / F07)
    F10,
    /// F-Series 1-Series / 2-Series (F20 / F21 / F22 / F23)
    F20,
    /// F-Series 3-Series / 4-Series (F30 / F31 / F32 / F33 / F36 / F80 / F82)
    F30,
    Other(String),
}

impl Chassis {
    /// Infers chassis from a model/chassis hint string.
    pub fn from_vin_or_hint(hint: &str) -> Self {
        let upper = hint.to_uppercase();
        if upper.contains("E46") {
            Self::E46
        } else if upper.contains("E81")
            || upper.contains("E82")
            || upper.contains("E87")
            || upper.contains("E88")
        {
            Self::E87
        } else if upper.contains("E90")
            || upper.contains("E91")
            || upper.contains("E92")
            || upper.contains("E93")
        {
            Self::E90
        } else if upper.contains("E60") || upper.contains("E61") {
            Self::E60
        } else if upper.contains("E84") {
            Self::E84
        } else if upper.contains("F10") || upper.contains("F11") || upper.contains("F07") {
            Self::F10
        } else if upper.contains("F20")
            || upper.contains("F21")
            || upper.contains("F22")
            || upper.contains("F23")
            || upper.contains("F87")
        {
            Self::F20
        } else if upper.contains("F30")
            || upper.contains("F31")
            || upper.contains("F32")
            || upper.contains("F33")
            || upper.contains("F36")
            || upper.contains("F80")
            || upper.contains("F82")
        {
            Self::F30
        } else {
            Self::Other(upper)
        }
    }

    /// Returns the shared ECU platform family (e.g. E87 & E90 share `E8xE9x`; F10/F20/F30 share `Fxx`).
    pub fn platform_family(&self) -> &'static str {
        match self {
            Chassis::E46 => "E46",
            Chassis::E87 | Chassis::E90 | Chassis::E84 => "E8xE9x",
            Chassis::E60 => "E60",
            Chassis::F10 | Chassis::F20 | Chassis::F30 => "Fxx",
            Chassis::Other(_) => "GENERIC",
        }
    }
}

impl fmt::Display for Chassis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Chassis::E46 => write!(f, "E46"),
            Chassis::E87 => write!(f, "E87 (1-Series)"),
            Chassis::E90 => write!(f, "E90 (3-Series)"),
            Chassis::E60 => write!(f, "E60 (5-Series)"),
            Chassis::E84 => write!(f, "E84 (X1)"),
            Chassis::F10 => write!(f, "F10 (5-Series)"),
            Chassis::F20 => write!(f, "F20 (1/2-Series)"),
            Chassis::F30 => write!(f, "F30 (3/4-Series)"),
            Chassis::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Identifies a BMW ECU module (e.g., LSZ, GM5, FRM, CAS, IHKA, KOMBI).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EcuInfo {
    /// Diagnostic address on the bus (e.g., 0xD0 for LSZ, 0x72 for FRM, 0x00 for CAS).
    pub address: u8,
    /// Short BMW module name (e.g., "LSZ", "FRM", "CAS", "IHKA").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Coding index (e.g., "LSZ.C37", "FRM2_E89.C08").
    pub coding_index: String,
}

/// Vehicle identification summary returned by `scan_vehicle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleInfo {
    pub vin: String,
    pub chassis: Chassis,
    pub battery_voltage: f32,
    pub ecus: Vec<EcuInfo>,
}

/// Single parameter change between current ECU state and target state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodingDiff {
    pub parameter: String,
    pub old_value: String,
    pub new_value: String,
}

/// A diagnostic trouble code (DTC / error memory entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dtc {
    pub ecu: String,
    pub code: String,
    pub description: String,
    pub active: bool,
}

/// A single parameter override within a coding preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetChange {
    pub parameter: String,
    pub value: String,
    /// Optional fallback parameter names used across different coding indices.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Curated or user-defined one-tap coding preset.
///
/// Presets are matched primarily by `target_ecu` and `platform_families` (e.g. `["E8xE9x"]`
/// covers 1-Series E81/E82/E87/E88, 3-Series E90/E91/E92/E93, and X1 E84).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingPreset {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Platform families this preset applies to (`"E46"`, `"E8xE9x"`, `"*"`, etc.).
    #[serde(default)]
    pub platform_families: Vec<String>,
    pub target_ecu: String,
    pub changes: Vec<PresetChange>,
}

impl CodingPreset {
    /// Returns true if the preset is compatible with the given chassis (or `"*"` wildcard).
    pub fn supports_chassis(&self, chassis: &Chassis) -> bool {
        if self.platform_families.is_empty() || self.platform_families.iter().any(|f| f == "*") {
            return true;
        }
        let family = chassis.platform_family();
        let chassis_str = format!("{chassis:?}").to_uppercase();
        self.platform_families
            .iter()
            .any(|f| f.eq_ignore_ascii_case(family) || f.eq_ignore_ascii_case(&chassis_str))
    }
}

/// Result of a safe coding operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingReport {
    pub ecu: String,
    pub backup_path: String,
    pub applied_diffs: Vec<CodingDiff>,
    pub verified: bool,
}

/// Metadata for a saved ECU coding backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub vin: String,
    pub ecu: String,
    pub timestamp: String,
    pub path: String,
}

use crate::error::{HandosError, Result};
use crate::nettodat::{CodingParamSchema, EcuCodingSchema};
use crate::presets::builtin_ecu_schemas;
use crate::types::EcuInfo;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Dynamically indexes ECU coding schemas by `coding_index` (e.g. `FRM2_E89.C08`)
/// from both built-in schemas and user-imported files on the device.
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    schemas: Vec<EcuCodingSchema>,
}

impl SchemaRegistry {
    /// Creates a registry pre-populated with built-in schemas, then loads any
    /// `.json` or `.xml` schema files found in `schemas_dir`.
    pub fn load(schemas_dir: impl AsRef<Path>) -> Result<Self> {
        let mut reg = Self {
            schemas: builtin_ecu_schemas(),
        };
        let dir = schemas_dir.as_ref();
        if !dir.exists() {
            fs::create_dir_all(dir)?;
            return Ok(reg);
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase());

            match ext.as_deref() {
                Some("json") => {
                    let raw = fs::read_to_string(&path)?;
                    if let Ok(list) = serde_json::from_str::<Vec<EcuCodingSchema>>(&raw) {
                        reg.schemas.extend(list);
                    } else if let Ok(single) = serde_json::from_str::<EcuCodingSchema>(&raw) {
                        reg.schemas.push(single);
                    }
                }
                Some("xml") => {
                    let raw = fs::read_to_string(&path)?;
                    if let Ok(schema) = parse_ncsdummy_xml(&raw) {
                        reg.schemas.push(schema);
                    }
                }
                _ => {}
            }
        }
        Ok(reg)
    }

    pub fn add_schema(&mut self, schema: EcuCodingSchema) {
        self.schemas.push(schema);
    }

    pub fn len(&self) -> usize {
        self.schemas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    /// Dynamically resolves the best matching `EcuCodingSchema` for a scanned ECU
    /// using its reported `coding_index` (exact match -> prefix match -> ECU name).
    pub fn resolve_for_ecu(&self, ecu: &EcuInfo) -> Option<&EcuCodingSchema> {
        let idx_upper = ecu.coding_index.to_uppercase();
        let ecu_upper = ecu.name.to_uppercase();

        // 1. Exact coding_index match
        if let Some(s) = self
            .schemas
            .iter()
            .rev()
            .find(|s| s.coding_index_prefix.to_uppercase() == idx_upper)
        {
            return Some(s);
        }

        // 2. Prefix match (e.g. "FRM2_E89" matches "FRM2_E89.C08")
        if let Some(s) = self.schemas.iter().rev().find(|s| {
            !s.coding_index_prefix.is_empty()
                && idx_upper.starts_with(&s.coding_index_prefix.to_uppercase())
        }) {
            return Some(s);
        }

        // 3. Fallback to ECU module name
        self.schemas
            .iter()
            .rev()
            .find(|s| s.ecu_name.to_uppercase() == ecu_upper)
    }
}

/// Parses an NCS Dummy-style XML module definition into an `EcuCodingSchema`.
///
/// Supports lightweight `<module ecu="FRM" coding_index="FRM3_E89.C32">` with
/// `<function name="SIDEMARKER_US" block="0x3000" offset="0" mask="0x01">` and
/// `<param name="aktiv" value="0x01"/>` children.
pub fn parse_ncsdummy_xml(xml: &str) -> Result<EcuCodingSchema> {
    let mut ecu_name = String::new();
    let mut coding_index_prefix = String::new();
    let mut params = Vec::new();
    let mut current_fn: Option<CodingParamSchema> = None;

    for raw_line in xml.lines() {
        let line = raw_line.trim();
        if line.starts_with("<module") {
            ecu_name = extract_attr(line, "ecu").unwrap_or_default();
            coding_index_prefix = extract_attr(line, "coding_index").unwrap_or_default();
        } else if line.starts_with("<function") {
            if let Some(prev) = current_fn.take() {
                params.push(prev);
            }
            let name = extract_attr(line, "name").ok_or_else(|| {
                HandosError::Protocol("XML <function> missing 'name' attribute".to_string())
            })?;
            let block_id = parse_u16_hex_or_dec(
                &extract_attr(line, "block").unwrap_or_else(|| "0x3000".to_string()),
            )?;
            let byte_offset = extract_attr(line, "offset")
                .unwrap_or_else(|| "0".to_string())
                .parse::<usize>()
                .map_err(|e| HandosError::Protocol(format!("invalid offset in XML: {e}")))?;
            let bitmask = parse_u8_hex_or_dec(
                &extract_attr(line, "mask").unwrap_or_else(|| "0xFF".to_string()),
            )?;

            current_fn = Some(CodingParamSchema {
                parameter: name,
                block_id,
                byte_offset,
                bitmask,
                values: BTreeMap::new(),
            });
        } else if line.starts_with("<param") {
            if let Some(ref mut fn_schema) = current_fn {
                if let (Some(name), Some(val_str)) =
                    (extract_attr(line, "name"), extract_attr(line, "value"))
                {
                    let val = parse_u8_hex_or_dec(&val_str)?;
                    fn_schema.values.insert(name, val);
                }
            }
        } else if line.starts_with("</function>") {
            if let Some(prev) = current_fn.take() {
                params.push(prev);
            }
        }
    }
    if let Some(prev) = current_fn.take() {
        params.push(prev);
    }

    if ecu_name.is_empty() && coding_index_prefix.is_empty() {
        return Err(HandosError::Protocol(
            "XML missing <module ecu=... coding_index=...>".to_string(),
        ));
    }

    Ok(EcuCodingSchema {
        ecu_name,
        coding_index_prefix,
        params,
    })
}

fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_u16_hex_or_dec(raw: &str) -> Result<u16> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
            .map_err(|e| HandosError::Protocol(format!("invalid hex '{s}': {e}")))
    } else {
        s.parse::<u16>()
            .map_err(|e| HandosError::Protocol(format!("invalid number '{s}': {e}")))
    }
}

fn parse_u8_hex_or_dec(raw: &str) -> Result<u8> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
            .map_err(|e| HandosError::Protocol(format!("invalid hex '{s}': {e}")))
    } else {
        s.parse::<u8>()
            .map_err(|e| HandosError::Protocol(format!("invalid number '{s}': {e}")))
    }
}

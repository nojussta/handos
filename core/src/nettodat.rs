use crate::error::{HandosError, Result};
use crate::fsw_psw::FswPswFile;
use crate::types::CodingDiff;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Raw binary coding memory image (`NETTODAT`), organized by block ID -> bytes.
///
/// BMW ECUs store coding data as raw byte blocks (not text). `NettodatImage`
/// preserves every untouched byte/bit when modifying specific coding parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NettodatImage {
    pub blocks: BTreeMap<u16, Vec<u8>>,
}

impl NettodatImage {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_single_block(block_id: u16, data: Vec<u8>) -> Self {
        let mut blocks = BTreeMap::new();
        blocks.insert(block_id, data);
        Self { blocks }
    }

    /// Parses a simplified BMW `NETTODAT.TRC` hex dump:
    /// ```text
    /// B 00300000,0010,00,01,FF,...
    /// ```
    pub fn parse_nettodat_trc(input: &str) -> Result<Self> {
        let mut blocks = BTreeMap::new();
        for (idx, raw_line) in input.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('B') {
                let parts: Vec<&str> = rest.trim().split(',').map(|s| s.trim()).collect();
                if parts.len() < 3 {
                    return Err(HandosError::Protocol(format!(
                        "invalid NETTODAT line {}: '{line}'",
                        idx + 1
                    )));
                }
                let addr = u32::from_str_radix(parts[0].trim_start_matches("0x"), 16)
                    .map_err(|e| HandosError::Protocol(format!("invalid block addr: {e}")))?;
                let block_id = (addr & 0xFFFF) as u16;
                let mut bytes = Vec::with_capacity(parts.len() - 2);
                for hex in &parts[2..] {
                    if hex.is_empty() {
                        continue;
                    }
                    let b = u8::from_str_radix(hex.trim_start_matches("0x"), 16)
                        .map_err(|e| HandosError::Protocol(format!("invalid hex '{hex}': {e}")))?;
                    bytes.push(b);
                }
                blocks
                    .entry(block_id)
                    .or_insert_with(Vec::new)
                    .extend(bytes);
            }
        }
        Ok(Self { blocks })
    }

    /// Serializes to `NETTODAT.TRC`-style hex dump for archival/debugging.
    pub fn to_nettodat_trc(&self) -> String {
        let mut out = String::new();
        for (&block_id, bytes) in &self.blocks {
            let hex_bytes: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            out.push_str(&format!(
                "B {:08X},{:04X},{}\n",
                block_id as u32,
                bytes.len(),
                hex_bytes.join(",")
            ));
        }
        out
    }

    /// Reads masked bits from `(block_id, byte_offset, bitmask)` shifted down to LSB.
    pub fn read_bits(&self, block_id: u16, byte_offset: usize, bitmask: u8) -> Result<u8> {
        let block = self.blocks.get(&block_id).ok_or_else(|| {
            HandosError::Protocol(format!("coding block 0x{block_id:04X} missing"))
        })?;
        let byte = *block.get(byte_offset).ok_or_else(|| {
            HandosError::Protocol(format!(
                "byte offset {byte_offset} out of range for block 0x{block_id:04X} (len {})",
                block.len()
            ))
        })?;
        if bitmask == 0 {
            return Err(HandosError::Protocol("bitmask cannot be 0".to_string()));
        }
        let shift = bitmask.trailing_zeros();
        Ok((byte & bitmask) >> shift)
    }

    /// Writes `value` into `(block_id, byte_offset, bitmask)` while preserving all unmasked bits.
    pub fn write_bits(
        &mut self,
        block_id: u16,
        byte_offset: usize,
        bitmask: u8,
        value: u8,
    ) -> Result<()> {
        let block = self.blocks.get_mut(&block_id).ok_or_else(|| {
            HandosError::Protocol(format!("coding block 0x{block_id:04X} missing"))
        })?;
        let byte = block.get_mut(byte_offset).ok_or_else(|| {
            HandosError::Protocol(format!(
                "byte offset {byte_offset} out of range for block 0x{block_id:04X}"
            ))
        })?;
        if bitmask == 0 {
            return Err(HandosError::Protocol("bitmask cannot be 0".to_string()));
        }
        let shift = bitmask.trailing_zeros();
        *byte = (*byte & !bitmask) | ((value << shift) & bitmask);
        Ok(())
    }
}

/// Maps a single BMW `FSW` parameter to its binary location inside `NettodatImage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingParamSchema {
    pub parameter: String,
    pub block_id: u16,
    pub byte_offset: usize,
    pub bitmask: u8,
    /// Map of `PSW` name (e.g. `"aktiv"`, `"nicht_aktiv"`, `"wert_02"`) -> raw masked value.
    pub values: BTreeMap<String, u8>,
}

/// Coding schema for an ECU family/coding index (equivalent to a parsed SP-Daten `.Cxx` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcuCodingSchema {
    pub ecu_name: String,
    pub coding_index_prefix: String,
    pub params: Vec<CodingParamSchema>,
}

impl EcuCodingSchema {
    /// Decodes a binary `NettodatImage` into a human-readable `FswPswFile`.
    pub fn decode(&self, image: &NettodatImage) -> Result<FswPswFile> {
        let mut fsw = FswPswFile::new();
        for p in &self.params {
            let raw_val = image.read_bits(p.block_id, p.byte_offset, p.bitmask)?;
            let psw = p
                .values
                .iter()
                .find(|(_, &v)| v == raw_val)
                .map(|(k, _)| k.clone())
                .unwrap_or_else(|| format!("raw_0x{raw_val:02X}"));
            fsw.set(p.parameter.clone(), psw);
        }
        Ok(fsw)
    }

    /// Encodes `FswPswFile` changes back onto an existing `NettodatImage`,
    /// preserving all unrelated bytes/bits and returning the applied diffs.
    pub fn encode_onto(
        &self,
        image: &mut NettodatImage,
        target: &FswPswFile,
    ) -> Result<Vec<CodingDiff>> {
        let current = self.decode(image)?;
        let mut diffs = Vec::new();

        for p in &self.params {
            if let Some(target_psw) = target.get_single(&p.parameter) {
                let current_psw = current.get_single(&p.parameter).unwrap_or("<unset>");
                if current_psw != target_psw {
                    let raw = p.values.get(target_psw).copied().ok_or_else(|| {
                        HandosError::Protocol(format!(
                            "unknown PSW value '{target_psw}' for parameter '{}'",
                            p.parameter
                        ))
                    })?;
                    image.write_bits(p.block_id, p.byte_offset, p.bitmask, raw)?;
                    diffs.push(CodingDiff {
                        parameter: p.parameter.clone(),
                        old_value: current_psw.to_string(),
                        new_value: target_psw.to_string(),
                    });
                }
            }
        }
        Ok(diffs)
    }
}

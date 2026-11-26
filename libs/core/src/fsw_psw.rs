use crate::error::{HandosError, Result};
use crate::types::{CodingDiff, PresetChange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Represents a parsed BMW `FSW_PSW.TRC` or `FSW_PSW.MAN` file.
///
/// Preserves parameter order in `order` while allowing O(log N) lookup by parameter name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FswPswFile {
    order: Vec<String>,
    entries: BTreeMap<String, Vec<String>>,
}

impl FswPswFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &BTreeMap<String, Vec<String>> {
        &self.entries
    }

    /// Parses BMW `FSW_PSW.TRC` / `FSW_PSW.MAN` text format.
    ///
    /// Format example:
    /// ```text
    /// SIDEMARKER_US
    ///     aktiv
    /// BLINKIMPULS_WARNBLK
    ///     wert_02
    /// ```
    pub fn parse(input: &str) -> Result<Self> {
        let mut file = Self::new();
        let mut current_param: Option<String> = None;

        for (idx, raw_line) in input.lines().enumerate() {
            let line_no = idx + 1;
            let trimmed_end = raw_line.trim_end();
            let trimmed = trimmed_end.trim();

            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with('*')
            {
                continue;
            }

            let is_indented = raw_line.starts_with('\t') || raw_line.starts_with(' ');
            if is_indented {
                let param = current_param
                    .as_ref()
                    .ok_or_else(|| HandosError::FswPswParse {
                        line: line_no,
                        message: format!("value '{trimmed}' found before any parameter name"),
                    })?;
                file.entries
                    .entry(param.clone())
                    .or_default()
                    .push(trimmed.to_string());
            } else {
                let param_name = trimmed.to_string();
                if !file.entries.contains_key(&param_name) {
                    file.order.push(param_name.clone());
                    file.entries.insert(param_name.clone(), Vec::new());
                }
                current_param = Some(param_name);
            }
        }

        Ok(file)
    }

    /// Serializes back to BMW `FSW_PSW.MAN` / `FSW_PSW.TRC` format.
    pub fn to_trc_string(&self) -> String {
        let mut out = String::new();
        for param in &self.order {
            out.push_str(param);
            out.push('\n');
            if let Some(values) = self.entries.get(param) {
                for val in values {
                    out.push('\t');
                    out.push_str(val);
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn get(&self, parameter: &str) -> Option<&[String]> {
        self.entries.get(parameter).map(|v| v.as_slice())
    }

    pub fn get_single(&self, parameter: &str) -> Option<&str> {
        self.entries
            .get(parameter)
            .and_then(|v| v.first())
            .map(|s| s.as_str())
    }

    pub fn set(&mut self, parameter: impl Into<String>, value: impl Into<String>) {
        let key = parameter.into();
        if !self.entries.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.entries.insert(key, vec![value.into()]);
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn parameters(&self) -> &[String] {
        &self.order
    }

    /// Applies preset changes (resolving aliases if the primary parameter name is absent
    /// in this ECU's coding index) and returns the list of actual diffs.
    pub fn apply_preset_changes(&mut self, changes: &[PresetChange]) -> Result<Vec<CodingDiff>> {
        self.apply_preset_changes_mode(changes, true)
    }

    /// Reverts preset changes back to each parameter's `off_value`.
    pub fn revert_preset_changes(&mut self, changes: &[PresetChange]) -> Result<Vec<CodingDiff>> {
        self.apply_preset_changes_mode(changes, false)
    }

    /// Returns true if every parameter in `changes` already equals its enabled `value`.
    pub fn is_preset_enabled(&self, changes: &[PresetChange]) -> bool {
        !changes.is_empty()
            && changes.iter().all(|c| {
                let name = self.resolve_parameter_name(&c.parameter, &c.aliases);
                self.get_single(&name) == Some(c.value.as_str())
            })
    }

    fn apply_preset_changes_mode(
        &mut self,
        changes: &[PresetChange],
        enable: bool,
    ) -> Result<Vec<CodingDiff>> {
        if self.is_empty() {
            return Err(HandosError::Protocol(
                "refusing to modify empty ECU coding trace".to_string(),
            ));
        }
        let mut diffs = Vec::new();
        for change in changes {
            let resolved_name = self
                .find_existing_parameter(&change.parameter, &change.aliases)
                .ok_or_else(|| HandosError::UnknownParameter {
                    parameter: change.parameter.clone(),
                })?;
            let target_val = if enable {
                &change.value
            } else {
                &change.off_value
            };
            let old_val = self
                .get_single(&resolved_name)
                .unwrap_or("<unset>")
                .to_string();

            if old_val != *target_val {
                self.set(resolved_name.clone(), target_val.clone());
                diffs.push(CodingDiff {
                    parameter: resolved_name,
                    old_value: old_val,
                    new_value: target_val.clone(),
                });
            }
        }
        Ok(diffs)
    }

    /// Computes diffs between `self` (current) and `target` without mutating `self`.
    pub fn diff_against(&self, target: &FswPswFile) -> Vec<CodingDiff> {
        let mut diffs = Vec::new();
        for param in &target.order {
            let new_val = target.get_single(param).unwrap_or("");
            let old_val = self.get_single(param).unwrap_or("<unset>");
            if old_val != new_val {
                diffs.push(CodingDiff {
                    parameter: param.clone(),
                    old_value: old_val.to_string(),
                    new_value: new_val.to_string(),
                });
            }
        }
        diffs
    }

    fn resolve_parameter_name(&self, primary: &str, aliases: &[String]) -> String {
        self.find_existing_parameter(primary, aliases)
            .unwrap_or_else(|| primary.to_string())
    }

    fn find_existing_parameter(&self, primary: &str, aliases: &[String]) -> Option<String> {
        if self.entries.contains_key(primary) {
            return Some(primary.to_string());
        }
        for alias in aliases {
            if self.entries.contains_key(alias) {
                return Some(alias.clone());
            }
        }
        None
    }
}

/// Minimal SP-Daten / NCS Dummy parameter metadata entry for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpDatenParamDef {
    pub name: String,
    pub description: String,
    pub possible_values: Vec<String>,
}

/// Parses a simplified SP-Daten / NCS Dummy text definition block (`FUNCTION_NAME // description | val1, val2`).
pub fn parse_sp_daten_defs(input: &str) -> Vec<SpDatenParamDef> {
    let mut defs = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Format: PARAM_NAME | Description | val1, val2
        let parts: Vec<&str> = trimmed.split('|').map(|s| s.trim()).collect();
        if parts.is_empty() || parts[0].is_empty() {
            continue;
        }
        let name = parts[0].to_string();
        let description = parts.get(1).copied().unwrap_or("").to_string();
        let possible_values = parts
            .get(2)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        defs.push(SpDatenParamDef {
            name,
            description,
            possible_values,
        });
    }
    defs
}

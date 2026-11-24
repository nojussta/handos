use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Single live engine/vehicle telemetry sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySample {
    pub timestamp_ms: i64,
    pub battery_voltage: f32,
    pub rpm: u16,
    pub coolant_temp_c: f32,
    pub oil_temp_c: f32,
    pub boost_kpa: f32,
    pub intake_temp_c: f32,
    pub vehicle_speed_kmh: f32,
}

/// Records `TelemetrySample` points and exports them to CSV files.
#[derive(Debug, Default)]
pub struct DataLogger {
    samples: Vec<TelemetrySample>,
}

impl DataLogger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, sample: TelemetrySample) {
        self.samples.push(sample);
    }

    pub fn samples(&self) -> &[TelemetrySample] {
        &self.samples
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn to_csv(&self) -> String {
        let mut out = String::from(
            "timestamp_ms,battery_voltage,rpm,coolant_temp_c,oil_temp_c,boost_kpa,intake_temp_c,vehicle_speed_kmh\n",
        );
        for s in &self.samples {
            out.push_str(&format!(
                "{},{:.2},{},{:.1},{:.1},{:.1},{:.1},{:.1}\n",
                s.timestamp_ms,
                s.battery_voltage,
                s.rpm,
                s.coolant_temp_c,
                s.oil_temp_c,
                s.boost_kpa,
                s.intake_temp_c,
                s.vehicle_speed_kmh
            ));
        }
        out
    }

    /// Writes the recorded samples to `<logs_dir>/handos_log_<timestamp>.csv`.
    pub fn save_csv(&self, logs_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let dir = logs_dir.as_ref();
        fs::create_dir_all(dir)?;
        let ts = Utc::now().format("%Y%m%d_%H%M%S");
        let path = dir.join(format!("handos_log_{ts}.csv"));
        fs::write(&path, self.to_csv())?;
        Ok(path)
    }

    /// Appends samples starting at `start_index` to `path` (creating with header if needed).
    /// Returns the new total sample count (`self.samples.len()`).
    pub fn append_csv(&self, path: impl AsRef<Path>, start_index: usize) -> Result<usize> {
        let target = path.as_ref();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let needs_header = !target.exists() || fs::metadata(target)?.len() == 0;
        let mut file = OpenOptions::new().create(true).append(true).open(target)?;
        if needs_header {
            writeln!(
                file,
                "timestamp_ms,battery_voltage,rpm,coolant_temp_c,oil_temp_c,boost_kpa,intake_temp_c,vehicle_speed_kmh"
            )?;
        }
        for s in self.samples.iter().skip(start_index) {
            writeln!(
                file,
                "{},{:.2},{},{:.1},{:.1},{:.1},{:.1},{:.1}",
                s.timestamp_ms,
                s.battery_voltage,
                s.rpm,
                s.coolant_temp_c,
                s.oil_temp_c,
                s.boost_kpa,
                s.intake_temp_c,
                s.vehicle_speed_kmh
            )?;
        }
        Ok(self.samples.len())
    }
}

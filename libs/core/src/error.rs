use thiserror::Error;

/// Errors returned by the Handos core engine.
#[derive(Debug, Error)]
pub enum HandosError {
    #[error("connection failed to {address}: {reason}")]
    Connection { address: String, reason: String },

    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("ECU {ecu} rejected request (NRC 0x{nrc:02X}): {message}")]
    NegativeResponse {
        ecu: String,
        nrc: u8,
        message: String,
    },

    #[error("battery voltage too low ({voltage:.2}V < {minimum:.2}V) for safe coding")]
    LowVoltage { voltage: f32, minimum: f32 },

    #[error("unsafe vehicle state for ECU write: {reason}")]
    UnsafeVehicleState { reason: String },

    #[error("FSW_PSW parse error at line {line}: {message}")]
    FswPswParse { line: usize, message: String },

    #[error("preset '{preset_id}' does not match chassis {chassis}")]
    ChassisMismatch { preset_id: String, chassis: String },

    #[error("parameter '{parameter}' (and aliases) not present in ECU coding data; refusing to write unknown parameter")]
    UnknownParameter { parameter: String },

    #[error("verification failed for {ecu}: parameter '{parameter}' expected '{expected}', got '{actual}' (rollback_attempted={rollback_ok})")]
    VerificationFailed {
        ecu: String,
        parameter: String,
        expected: String,
        actual: String,
        rollback_ok: bool,
    },

    #[error("backup error: {0}")]
    Backup(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, HandosError>;

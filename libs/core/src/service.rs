use serde::{Deserialize, Serialize};

/// Common BMW maintenance/service routines (replacing INPA/ISTA service jobs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceJob {
    RegisterBattery,
    ResetOilService,
    ResetBrakeService,
}

impl ServiceJob {
    pub fn routine_id(self) -> u8 {
        match self {
            Self::RegisterBattery => 0x01,
            Self::ResetOilService => 0x02,
            Self::ResetBrakeService => 0x03,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::RegisterBattery => "Register New Battery",
            Self::ResetOilService => "Reset Oil Service (CBS)",
            Self::ResetBrakeService => "Reset Brake Service (CBS)",
        }
    }
}

/// Result of executing a BMW service routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceReport {
    pub job: ServiceJob,
    pub ecu: String,
    pub status: String,
}

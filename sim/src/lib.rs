use handos_core::fsw_psw::FswPswFile;
use handos_core::protocol::{sid, DiagnosticFrame, PayloadType, GATEWAY_ADDRESS, TESTER_ADDRESS};
use handos_core::types::{Chassis, Dtc, EcuInfo, VehicleInfo};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// In-memory simulated BMW vehicle state for testing without a physical car.
#[derive(Debug, Clone)]
pub struct SimulatedCar {
    pub vehicle: VehicleInfo,
    pub coding_data: HashMap<u8, FswPswFile>,
    pub dtcs: HashMap<u8, Vec<Dtc>>,
}

impl SimulatedCar {
    pub fn e46() -> Self {
        let mut coding_data = HashMap::new();
        coding_data.insert(
            0xD0,
            FswPswFile::parse(
                "SIDEMARKER_US\n\tnicht_aktiv\nBLK_V_GED_ALS_PARKLICHT\n\tnicht_aktiv\nBLINKIMPULS_WARNBLK\n\twert_01\nTAGFAHRLICHT_ECE\n\tnicht_aktiv\n",
            )
            .unwrap(),
        );
        coding_data.insert(
            0x00,
            FswPswFile::parse(
                "KOMFORTOEFFNUNG_FB\n\tnicht_aktiv\nKOMFORTSCHLIESSUNG_FB\n\tnicht_aktiv\nVERRIEGELN_AUT_AB_X_KMH\n\tnicht_aktiv\n",
            )
            .unwrap(),
        );

        let mut dtcs = HashMap::new();
        dtcs.insert(
            0xD0,
            vec![Dtc {
                ecu: "LSZ".to_string(),
                code: "0x0028".to_string(),
                description: "Front right parking light bulb intermittent".to_string(),
                active: false,
            }],
        );
        dtcs.insert(0x00, vec![]);

        Self {
            vehicle: VehicleInfo {
                vin: "WBAAV31040JH12345".to_string(),
                chassis: Chassis::E46,
                battery_voltage: 12.6,
                ecus: vec![
                    EcuInfo {
                        address: 0xD0,
                        name: "LSZ".to_string(),
                        description: "Light Switching Center".to_string(),
                        coding_index: "LSZ.C37".to_string(),
                    },
                    EcuInfo {
                        address: 0x00,
                        name: "GM5".to_string(),
                        description: "General Module V".to_string(),
                        coding_index: "GM5.C05".to_string(),
                    },
                ],
            },
            coding_data,
            dtcs,
        }
    }

    pub fn e87() -> Self {
        let mut car = Self::e90();
        car.vehicle.vin = "WBAUF51040J987654".to_string();
        car.vehicle.chassis = Chassis::E87;
        car
    }

    pub fn e90() -> Self {
        let mut coding_data = HashMap::new();
        coding_data.insert(
            0x72,
            FswPswFile::parse(
                "SIDEMARKER_US\n\tnicht_aktiv\nPWM_SL_V_BEI_AKTIVEN_AL\n\twert_01\nDOPPELBLINKIMPULS_WARNBLK\n\tnicht_aktiv\nWELCOME_LIGHT_ACTIVE\n\taktiv\n",
            )
            .unwrap(),
        );
        coding_data.insert(
            0x78,
            FswPswFile::parse("MSA_MEMORY\n\tnicht_aktiv\nUMLUFT_MEMORY\n\taktiv\n").unwrap(),
        );
        coding_data.insert(
            0x60,
            FswPswFile::parse("BC_DIGITAL_V\n\tnicht_aktiv\nBC_DIGITAL_V_KORREKTUR\n\taktiv\n")
                .unwrap(),
        );

        let mut dtcs = HashMap::new();
        dtcs.insert(
            0x72,
            vec![Dtc {
                ecu: "FRM".to_string(),
                code: "A8AB".to_string(),
                description: "Dipped beam right faulty (historic)".to_string(),
                active: false,
            }],
        );
        dtcs.insert(0x78, vec![]);
        dtcs.insert(0x60, vec![]);

        Self {
            vehicle: VehicleInfo {
                vin: "WBAVA31080NL54321".to_string(),
                chassis: Chassis::E90,
                battery_voltage: 12.8,
                ecus: vec![
                    EcuInfo {
                        address: 0x72,
                        name: "FRM".to_string(),
                        description: "Footwell Module (NFRM)".to_string(),
                        coding_index: "FRM2_E89.C08".to_string(),
                    },
                    EcuInfo {
                        address: 0x78,
                        name: "IHKA".to_string(),
                        description: "Integrated Automatic Heating/AC".to_string(),
                        coding_index: "IHKA_E89.C09".to_string(),
                    },
                    EcuInfo {
                        address: 0x60,
                        name: "KOMBI".to_string(),
                        description: "Instrument Cluster".to_string(),
                        coding_index: "KOMB87.C08".to_string(),
                    },
                ],
            },
            coding_data,
            dtcs,
        }
    }

    pub fn f30() -> Self {
        let mut coding_data = HashMap::new();
        coding_data.insert(
            0x40,
            FswPswFile::parse(
                "TCM_MSA_MEMORY\n\tnicht_aktiv\nTCM_MSA_DEFAULT_OFF\n\tnicht_aktiv\nASP_BEIKLAPPEN_BEI_KOMFORTSCHLIESSEN\n\tnicht_aktiv\nSIDEMARKER_US\n\tnicht_aktiv\n",
            )
            .unwrap(),
        );
        coding_data.insert(
            0x60,
            FswPswFile::parse("BC_DIGITAL_V\n\tnicht_aktiv\n").unwrap(),
        );
        coding_data.insert(
            0x63,
            FswPswFile::parse(
                "SPEEDLOCK_X_KMH_MAX\n\twerte=03\nCARLIFE\n\tnicht_aktiv\nWLAN_STATUS\n\tnicht_aktiv\nCARPLAY_FULLSCREEN\n\tnicht_aktiv\n",
            )
            .unwrap(),
        );

        let mut dtcs = HashMap::new();
        dtcs.insert(0x40, vec![]);
        dtcs.insert(0x60, vec![]);
        dtcs.insert(0x63, vec![]);

        Self {
            vehicle: VehicleInfo {
                vin: "WBA3B1C50DF123456".to_string(),
                chassis: Chassis::F30,
                battery_voltage: 12.7,
                ecus: vec![
                    EcuInfo {
                        address: 0x40,
                        name: "FEM_BODY".to_string(),
                        description: "Front Electronic Module".to_string(),
                        coding_index: "FEM_01.001".to_string(),
                    },
                    EcuInfo {
                        address: 0x60,
                        name: "KOMBI".to_string(),
                        description: "Instrument Cluster".to_string(),
                        coding_index: "KOMBI_F.001".to_string(),
                    },
                    EcuInfo {
                        address: 0x63,
                        name: "HU_NBT".to_string(),
                        description: "Head Unit NBT".to_string(),
                        coding_index: "HU_NBT.001".to_string(),
                    },
                ],
            },
            coding_data,
            dtcs,
        }
    }

    pub fn handle_frame(&mut self, frame: &DiagnosticFrame) -> DiagnosticFrame {
        if frame.payload.is_empty() {
            return DiagnosticFrame::new(
                PayloadType::Error,
                GATEWAY_ADDRESS,
                TESTER_ADDRESS,
                vec![sid::NEGATIVE_RESPONSE, 0x00, 0x13],
            );
        }

        let service = frame.payload[0];
        if frame.payload_type == PayloadType::AdapterControl && service == sid::ADAPTER_SCAN {
            let mut body = vec![service + 0x40];
            body.extend_from_slice(&serde_json::to_vec(&self.vehicle).unwrap());
            return DiagnosticFrame::new(
                PayloadType::AdapterControl,
                GATEWAY_ADDRESS,
                TESTER_ADDRESS,
                body,
            );
        }

        let target = frame.target;
        match service {
            sid::READ_CODING_DATA => {
                if let Some(trc) = self.coding_data.get(&target) {
                    let mut body = vec![service + 0x40];
                    body.extend_from_slice(trc.to_trc_string().as_bytes());
                    DiagnosticFrame::new(PayloadType::Diagnostic, target, TESTER_ADDRESS, body)
                } else {
                    neg(target, service, 0x31)
                }
            }
            sid::WRITE_CODING_DATA => {
                if !self.coding_data.contains_key(&target) {
                    return neg(target, service, 0x31);
                }
                let text = match std::str::from_utf8(&frame.payload[2..]) {
                    Ok(t) => t,
                    Err(_) => return neg(target, service, 0x13),
                };
                match FswPswFile::parse(text) {
                    Ok(parsed) => {
                        self.coding_data.insert(target, parsed);
                        DiagnosticFrame::new(
                            PayloadType::Diagnostic,
                            target,
                            TESTER_ADDRESS,
                            vec![service + 0x40, 0x01],
                        )
                    }
                    Err(_) => neg(target, service, 0x13),
                }
            }
            sid::READ_DTC => {
                let list = self.dtcs.get(&target).cloned().unwrap_or_default();
                let mut body = vec![service + 0x40];
                body.extend_from_slice(&serde_json::to_vec(&list).unwrap());
                DiagnosticFrame::new(PayloadType::Diagnostic, target, TESTER_ADDRESS, body)
            }
            sid::CLEAR_DTC => {
                if let Some(list) = self.dtcs.get_mut(&target) {
                    list.clear();
                }
                DiagnosticFrame::new(
                    PayloadType::Diagnostic,
                    target,
                    TESTER_ADDRESS,
                    vec![service + 0x40],
                )
            }
            _ => neg(target, service, 0x11),
        }
    }
}

fn neg(source: u8, service: u8, nrc: u8) -> DiagnosticFrame {
    DiagnosticFrame::new(
        PayloadType::Diagnostic,
        source,
        TESTER_ADDRESS,
        vec![sid::NEGATIVE_RESPONSE, service, nrc],
    )
}

/// Starts a TCP MHD WiFi simulator on `bind_addr` (e.g., `127.0.0.1:6801` or `127.0.0.1:0`).
pub fn spawn_simulator(bind_addr: &str, car: SimulatedCar) -> std::io::Result<String> {
    let listener = TcpListener::bind(bind_addr)?;
    let local_addr = listener.local_addr()?.to_string();
    let state = Arc::new(Mutex::new(car));

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&state);
            thread::spawn(move || handle_client(stream, state));
        }
    });

    Ok(local_addr)
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<SimulatedCar>>) {
    while let Ok(req) = DiagnosticFrame::read_from(&mut stream) {
        let resp = {
            let mut guard = state.lock().unwrap();
            guard.handle_frame(&req)
        };
        if resp.write_to(&mut stream).is_err() {
            break;
        }
    }
}

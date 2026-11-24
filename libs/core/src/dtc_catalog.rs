/// Offline BMW & OBD-II DTC description catalog so fault codes resolve
/// without internet access while connected to the offline MHD WiFi AP.
pub fn resolve_dtc_description(code: &str, existing: &str) -> String {
    let normalized = code.trim().trim_start_matches("0x").to_uppercase();
    let known = match normalized.as_str() {
        // Common BMW E46 / E8x / E9x / Fxx hex codes
        "0028" => Some("Front right parking/sidelight bulb circuit intermittent"),
        "A8AB" => Some("Dipped beam right faulty (FRM)"),
        "A8AC" => Some("Dipped beam left faulty (FRM)"),
        "9CB4" => Some("Ride height sensor front/rear signal implausible"),
        "2AAF" => Some("Fuel pump plausibility (HPFP / LPFP)"),
        "2ABC" => Some("Charging pressure sensor, electrical"),
        "29DC" => Some("Cylinder injection cutout / fuel pressure too low"),
        "2E81" => Some("Electric coolant pump, speed deviation"),
        "2E82" => Some("Electric coolant pump, cutoff"),
        "2E83" => Some("Electric coolant pump, power-reduced operation"),
        "2E85" => Some("Electric coolant pump, communication"),
        "30FF" => Some("Boost pressure below target (underboost)"),
        "3100" => Some("Boost pressure control deactivated (limp mode)"),
        "8040B9" => Some("Terminal 30B deactivation: startability limit reached"),
        "B7F8C3" => Some("WLAN antenna / head-unit connection fault"),
        // Standard OBD-II codes
        "P0171" => Some("System Too Lean (Bank 1)"),
        "P0174" => Some("System Too Lean (Bank 2)"),
        "P0300" => Some("Random/Multiple Cylinder Misfire Detected"),
        "P0301" => Some("Cylinder 1 Misfire Detected"),
        "P0302" => Some("Cylinder 2 Misfire Detected"),
        "P0303" => Some("Cylinder 3 Misfire Detected"),
        "P0304" => Some("Cylinder 4 Misfire Detected"),
        "P0420" => Some("Catalyst System Efficiency Below Threshold (Bank 1)"),
        _ => None,
    };

    if let Some(desc) = known {
        desc.to_string()
    } else if !existing.trim().is_empty() {
        existing.to_string()
    } else {
        format!("Unknown BMW/OBD fault code ({code})")
    }
}

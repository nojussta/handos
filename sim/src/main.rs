use handos_sim::{spawn_simulator, SimulatedCar};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut chassis = "e90".to_string();
    let mut addr = "127.0.0.1:6801".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--chassis" if i + 1 < args.len() => {
                chassis = args[i + 1].to_lowercase();
                i += 2;
            }
            "--bind" if i + 1 < args.len() => {
                addr = args[i + 1].clone();
                i += 2;
            }
            _ => i += 1,
        }
    }

    let car = match chassis.as_str() {
        "e46" => SimulatedCar::e46(),
        "e87" => SimulatedCar::e87(),
        "e90" => SimulatedCar::e90(),
        "f30" => SimulatedCar::f30(),
        other => {
            eprintln!(
                "Unknown chassis '{other}', defaulting to e90 (supported: e46, e87, e90, f30)"
            );
            SimulatedCar::e90()
        }
    };

    let bound = spawn_simulator(&addr, car.clone()).expect("failed to start MHD simulator");
    println!(
        "Handos MHD WiFi Simulator running at {bound} ({} | VIN {})",
        car.vehicle.chassis, car.vehicle.vin
    );
    println!("Press Ctrl+C to stop.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

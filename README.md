# Handos — Handy BMW Coding Tools

**Handos** replaces the legacy Windows laptop stack (**INPA + NCS Expert + NCS Dummy**) with a lightweight, safety-first mobile application for **Android** (and future **iOS**) using the **MHD Universal WiFi adapter**.

## Why Handos?

Instead of:
> Connect NCS Expert → Read VIN → Select ECU → Read ECU → Export `FSW_PSW.TRC` → Open NCS Dummy → Pick ECU → Tick checkboxes → Export `FSW_PSW.MAN` → Switch job → Code ECU

Handos gives you **one-tap, safety-verified coding presets** (plus an expert `FSW_PSW` / SP-Daten editor and DTC reader/clearer) with:
- **Automatic Pre-Coding Backups**: Every ECU read is automatically archived by VIN and timestamp before any write is allowed.
- **Diff Preview & Post-Write Verification**: See exact `old -> new` parameter changes before coding, and automatically re-read the ECU afterwards to verify.
- **One-Tap Presets**:
  - **E46 & E90**: USDM Corner Lights, Double Hazard Lights
  - **E90**: Auto Start/Stop Memory (`MSA_MEMORY`), Digital Speedometer
  - **Fault Codes**: Read & clear error memory (`FS_LESEN` / `FS_LOESCHEN`)
- **SP-Daten + Custom Presets**: Built-in curated presets plus import support for `FSW_PSW.TRC` / `FSW_PSW.MAN` and SP-Daten definitions.

## Architecture

- [`core/`](core/) (`handos-core` — Rust): Lightweight, memory-safe shared engine compiled to native Android (JNI/UniFFI) and iOS (Swift/UniFFI). Contains MHD WiFi TCP transport, BMW KWP2000/DS2/ENET framing, `FSW_PSW` parser/writer, preset engine, automatic backup manager, and safe coding state machine.
- [`sim/`](sim/) (`handos-sim` — Rust): Local MHD WiFi + BMW ECU simulator (E46 & E90 ECUs) so you can test every coding, backup, restore, and fault-clearing flow without a physical car.
- [`cli/`](cli/) (`handos-cli` — Rust): Terminal client for testing `handos-core` directly against `handos-sim` or a real MHD WiFi adapter.
- [`presets/`](presets/): Curated JSON coding presets and ECU parameter definitions for E46 and E90.
- [`android/`](android/): Native Android app (Kotlin + Jetpack Compose + Material 3) wired to `handos-core`.
- [`ios/`](ios/): iOS SwiftUI integration guide & UniFFI binding setup sharing the exact same `handos-core` library.

## Quick Start (Simulator + CLI)

```bash
# Run all core & integration tests
cargo test

# Terminal 1: Start the E90 (or E46) MHD WiFi car simulator
cargo run -p handos-sim -- --chassis e90

# Terminal 2: Run the CLI against the simulator
cargo run -p handos-cli -- --host 127.0.0.1:6801 scan
cargo run -p handos-cli -- --host 127.0.0.1:6801 apply-preset e90_usdm_corners
cargo run -p handos-cli -- --host 127.0.0.1:6801 dtc-read
cargo run -p handos-cli -- --host 127.0.0.1:6801 dtc-clear
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for protocol details, MHD WiFi framing, and Android/iOS build instructions.

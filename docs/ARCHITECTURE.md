# Handos Architecture & Protocol Design

## 1. Tech Stack Choice (Lightweight + Android & Future iOS)

| Layer | Technology | Why |
|-------|-----------|-----|
| **Core Engine** | **Rust (`handos-core`)** | Zero GC, tiny binary (~1-2 MB), memory-safe, compiles natively for Android (`aarch64-linux-android` via `cargo-ndk` / UniFFI) and iOS (`aarch64-apple-ios` via UniFFI). Zero duplicated protocol/coding logic. |
| **Android UI** | **Kotlin + Jetpack Compose + Material 3** | Native Android performance, modern declarative UI, direct WiFi network binding (`ConnectivityManager.bindProcessToNetwork`) for MHD WiFi AP without cellular routing issues. |
| **iOS UI (Future)** | **SwiftUI + UniFFI** | Shares 100% of `handos-core` via generated Swift bindings. |
| **Presets & Data** | **JSON Presets + `.TRC`/`.MAN` + SP-Daten** | Human-readable curated presets in `presets/`, plus native parser for BMW `FSW_PSW.TRC` / `FSW_PSW.MAN` and SP-Daten ECU coding indices. |

## 2. MHD WiFi Adapter Communication

The MHD Universal WiFi adapter exposes a TCP bridge (typically `192.168.4.1:6801` when connected to the `MHD_...` Wi-Fi AP):
- **ENET / DoIP / K+DCAN Bridge**: Frames are exchanged over TCP. Handos abstracts this behind the `Transport` trait in `core/src/transport.rs`, supporting:
  - Length-prefixed BMW ENET-style framing (`[4-byte BE length][2-byte payload type][source][target][payload]`)
  - Raw KWP2000 / DS2 framing for K-Line / D-CAN ECUs (E46 / early E90)

### Important Android Networking Note
When an Android phone connects to the MHD WiFi AP (which has no internet access), Android may route TCP traffic over cellular data instead of WiFi unless the app binds the socket to the WiFi `Network` via `ConnectivityManager.bindProcessToNetwork()` or `network.socketFactory`. The Android layer handles this before opening the TCP connection.

## 3. Safe Coding State Machine (`CodingSession`)

Every coding operation enforces this sequence:
1. **Connect & Health Check**: Verify adapter link and battery voltage (warn below `12.0 V`, block coding below `11.5 V` unless overridden).
2. **Identify Vehicle**: Read VIN and chassis code (e.g., `E46`, `E90`) and list reachable ECUs.
3. **Read ECU (`CODIERDATEN_LESEN`)**: Fetch current ECU coding trace (`FSW_PSW.TRC` representation).
4. **Automatic Backup**: Persist `<backup_dir>/<VIN>/<ECU>_<YYYYMMDD_HHMMSS>.trc` before any modification.
5. **Compute Diff**: Apply preset or manual edits in memory; produce a strict `Vec<CodingDiff>` (`parameter`, `old_value`, `new_value`).
6. **Write ECU (`CODIERDATEN_SCHREIBEN`)**: Send `FSW_PSW.MAN` changes to the ECU.
7. **Verify**: Re-read the ECU and confirm every modified parameter matches the target value.
8. **Optional Fault Clear**: Clear transient communication/coding DTCs if requested.

# Handos iOS Integration (Future Coverage)

`handos-core` is pure Rust with zero platform-specific dependencies, so iOS shares 100% of the BMW protocol, `FSW_PSW` parser, preset engine, and backup/verification logic.

## Generating Swift Bindings

1. Add `uniffi` to `core/Cargo.toml` and expose `HandosSession` via UniFFI.
2. Build static libraries for iOS targets:
   ```bash
   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
   cargo build --release --target aarch64-apple-ios -p handos-core
   cargo build --release --target aarch64-apple-ios-sim -p handos-core
   ```
3. Generate Swift bindings and package as an `XCFramework` for Xcode / SwiftUI.
4. On iOS, connect to the MHD WiFi AP (`192.168.4.1:6801`) using `NEHotspotConfiguration` (or system Wi-Fi settings) and call `HandosSession` directly from SwiftUI.

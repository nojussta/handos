# BMW F-Series NBT EVO (ID5/ID6) Android Auto Integration

BMW's factory NBT EVO (ID5/ID6) head unit does not include a built-in Android Auto receiver; it ships with Apple CarPlay and Baidu CarLife.

Open-source community projects such as:
- `Sq00ky/Supra-iDrive6-Android-Auto` (native QNX Android Auto daemon for iDrive 5/6)
- `nbt-aa-tile` (hooks the native CarLife slot to show an Android Auto menu tile)

work in **two stages**:

1. **OBD / FDL Coding (Handled by Handos)**
   - `fxx_nbt_evo_aa_prep` enables `CARLIFE` and `WLAN_STATUS` on `HU_NBT` / `HU_NBT2`.
   - `fxx_nbt_evo_carplay_fullscreen` enables `CARPLAY_FULLSCREEN` on `HU_NBT2`.
2. **Head-Unit QNX Binary Installation (Requires Root/SSH over ENET)**
   - Because standard OBD coding (`FSW_PSW` / FDL) cannot push executable binaries onto the QNX filesystem, installing the Android Auto receiver daemon itself still requires unlocking SSH/root access on the NBT EVO over ENET and deploying the binaries from the upstream project.

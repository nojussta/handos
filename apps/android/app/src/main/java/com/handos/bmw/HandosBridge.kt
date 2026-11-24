package com.handos.bmw

/**
 * Kotlin JNI interface to `handos-core` (`libhandos_core.so`).
 * All native calls return JSON: `{"ok": true, "data": ...}` or `{"ok": false, "error": "..."}`.
 */
object HandosBridge {
    const val DEFAULT_MHD_HOST = "192.168.4.1:6801"

    val isNativeLoaded: Boolean = try {
        System.loadLibrary("handos_core")
        true
    } catch (_: UnsatisfiedLinkError) {
        false
    }

    external fun nativeScanVehicle(host: String, backupDir: String): String
    external fun nativeApplyPreset(host: String, presetId: String, backupDir: String): String
    external fun nativeRevertPreset(host: String, presetId: String, backupDir: String): String
    external fun nativePresetStatuses(host: String, backupDir: String): String
    external fun nativeReadDtcs(host: String, backupDir: String): String
    external fun nativeClearDtcs(host: String, backupDir: String): String
    external fun nativeListBackups(vin: String, backupDir: String): String
    external fun nativeRestoreBackup(host: String, ecuName: String, backupPath: String, backupDir: String): String
    external fun nativeRunServiceJob(host: String, jobId: String, backupDir: String): String
    external fun nativeReadTelemetry(host: String, backupDir: String): String
    external fun nativeReadEcuParams(host: String, ecuName: String, backupDir: String): String
    external fun nativeSetEcuParam(host: String, ecuName: String, parameter: String, value: String, backupDir: String): String
}

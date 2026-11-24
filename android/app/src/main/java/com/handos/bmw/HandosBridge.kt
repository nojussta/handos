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
    external fun nativeReadDtcs(host: String, backupDir: String): String
    external fun nativeClearDtcs(host: String, backupDir: String): String
    external fun nativeImportSchema(schemasDir: String, fileName: String, content: String): String
}

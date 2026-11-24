use crate::backup::BackupManager;
use crate::presets::builtin_presets;
use crate::session::HandosSession;
use crate::transport::MhdWifiTransport;
use serde::Serialize;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::time::Duration;

#[derive(Serialize)]
struct FfiResponse<T: Serialize> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

fn to_json_response<T: Serialize>(res: crate::Result<T>) -> String {
    let payload = match res {
        Ok(val) => FfiResponse {
            ok: true,
            data: Some(val),
            error: None,
        },
        Err(err) => FfiResponse {
            ok: false,
            data: None,
            error: Some(err.to_string()),
        },
    };
    serde_json::to_string(&payload)
        .unwrap_or_else(|e| format!("{{\"ok\":false,\"error\":\"{e}\"}}"))
}

pub fn scan_vehicle_json(host: &str, backup_dir: &str) -> String {
    let res = (|| {
        let transport = MhdWifiTransport::connect(host, Duration::from_secs(5))?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        session.scan_vehicle()
    })();
    to_json_response(res)
}

pub fn apply_preset_json(host: &str, preset_id: &str, backup_dir: &str) -> String {
    set_preset_enabled_json(host, preset_id, backup_dir, true)
}

pub fn revert_preset_json(host: &str, preset_id: &str, backup_dir: &str) -> String {
    set_preset_enabled_json(host, preset_id, backup_dir, false)
}

fn set_preset_enabled_json(host: &str, preset_id: &str, backup_dir: &str, enable: bool) -> String {
    let res = (|| {
        let preset = builtin_presets()
            .into_iter()
            .find(|p| p.id == preset_id)
            .ok_or_else(|| crate::HandosError::Protocol(format!("unknown preset '{preset_id}'")))?;
        let transport = MhdWifiTransport::connect(host, Duration::from_secs(5))?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        if enable {
            session.apply_preset_safely(&preset)
        } else {
            session.revert_preset_safely(&preset)
        }
    })();
    to_json_response(res)
}

pub fn preset_statuses_json(host: &str, backup_dir: &str) -> String {
    let res = (|| {
        let transport = MhdWifiTransport::connect(host, Duration::from_secs(5))?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        session.check_presets_status()
    })();
    to_json_response(res)
}

pub fn read_dtcs_json(host: &str, backup_dir: &str) -> String {
    let res = (|| {
        let transport = MhdWifiTransport::connect(host, Duration::from_secs(5))?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        let info = session.scan_vehicle()?;
        let mut all = Vec::new();
        for ecu in &info.ecus {
            all.extend(session.read_dtcs(ecu)?);
        }
        Ok(all)
    })();
    to_json_response(res)
}

pub fn clear_dtcs_json(host: &str, backup_dir: &str) -> String {
    let res = (|| {
        let transport = MhdWifiTransport::connect(host, Duration::from_secs(5))?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        let info = session.scan_vehicle()?;
        for ecu in &info.ecus {
            session.clear_dtcs(ecu)?;
        }
        Ok(info.ecus.len())
    })();
    to_json_response(res)
}

pub fn import_schema_json(schemas_dir: &str, file_name: &str, content: &str) -> String {
    let res = (|| -> crate::Result<usize> {
        let dir = std::path::Path::new(schemas_dir);
        std::fs::create_dir_all(dir)?;
        let safe_name: String = file_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        std::fs::write(dir.join(safe_name), content)?;
        let reg = crate::schema_registry::SchemaRegistry::load(dir)?;
        Ok(reg.len())
    })();
    to_json_response(res)
}

// --- C-ABI exports for iOS Swift / cross-platform FFI ---

/// # Safety
/// `host` and `backup_dir` must be valid, non-null, NUL-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn handos_scan_vehicle(
    host: *const c_char,
    backup_dir: *const c_char,
) -> *mut c_char {
    let host_s = CStr::from_ptr(host).to_string_lossy();
    let dir_s = CStr::from_ptr(backup_dir).to_string_lossy();
    CString::new(scan_vehicle_json(&host_s, &dir_s))
        .unwrap()
        .into_raw()
}

/// # Safety
/// `host`, `preset_id`, and `backup_dir` must be valid, non-null, NUL-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn handos_apply_preset(
    host: *const c_char,
    preset_id: *const c_char,
    backup_dir: *const c_char,
) -> *mut c_char {
    let host_s = CStr::from_ptr(host).to_string_lossy();
    let id_s = CStr::from_ptr(preset_id).to_string_lossy();
    let dir_s = CStr::from_ptr(backup_dir).to_string_lossy();
    CString::new(apply_preset_json(&host_s, &id_s, &dir_s))
        .unwrap()
        .into_raw()
}

/// # Safety
/// `ptr` must be a pointer returned by a `handos_*` C-ABI function, or null.
#[no_mangle]
pub unsafe extern "C" fn handos_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

// --- Android JNI exports ---

#[cfg(target_os = "android")]
mod android_jni {
    use super::*;
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    fn get_str(env: &mut JNIEnv, s: JString) -> String {
        env.get_string(&s)
            .map(|js| js.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeScanVehicle(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let d = get_str(&mut env, backup_dir);
        let out = scan_vehicle_json(&h, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeApplyPreset(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        preset_id: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let p = get_str(&mut env, preset_id);
        let d = get_str(&mut env, backup_dir);
        let out = apply_preset_json(&h, &p, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeRevertPreset(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        preset_id: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let p = get_str(&mut env, preset_id);
        let d = get_str(&mut env, backup_dir);
        let out = revert_preset_json(&h, &p, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativePresetStatuses(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let d = get_str(&mut env, backup_dir);
        let out = preset_statuses_json(&h, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeReadDtcs(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let d = get_str(&mut env, backup_dir);
        let out = read_dtcs_json(&h, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeClearDtcs(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let d = get_str(&mut env, backup_dir);
        let out = clear_dtcs_json(&h, &d);
        env.new_string(out).unwrap().into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeImportSchema(
        mut env: JNIEnv,
        _class: JClass,
        schemas_dir: JString,
        file_name: JString,
        content: JString,
    ) -> jstring {
        let dir = get_str(&mut env, schemas_dir);
        let name = get_str(&mut env, file_name);
        let body = get_str(&mut env, content);
        let out = import_schema_json(&dir, &name, &body);
        env.new_string(out).unwrap().into_raw()
    }
}

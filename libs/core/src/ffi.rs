use crate::backup::BackupManager;
use crate::error::HandosError;
use crate::presets::builtin_presets;
use crate::session::HandosSession;
use crate::transport::{KdcanProtocolMode, KdcanSerialTransport, MhdWifiTransport, Transport};
use serde::Serialize;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn connect_transport(endpoint: &str) -> crate::Result<Box<dyn Transport>> {
    if let Some(rest) = endpoint.strip_prefix("kdcan-fd://") {
        #[cfg(unix)]
        {
            let (fd_str, query) = rest.split_once('?').unwrap_or((rest, ""));
            let fd: i32 = fd_str.trim().parse().map_err(|_| HandosError::Connection {
                address: endpoint.to_string(),
                reason: "invalid USB file descriptor".to_string(),
            })?;
            if fd < 0 {
                return Err(HandosError::Connection {
                    address: endpoint.to_string(),
                    reason: "negative USB file descriptor".to_string(),
                });
            }
            let mode = if query.contains("baud=10400") {
                KdcanProtocolMode::Kline10400
            } else {
                KdcanProtocolMode::Dcan115200
            };
            // SAFETY: The caller (Android UsbDeviceConnection) keeps `fd` open for the duration of the FFI call.
            let stream = unsafe { crate::transport::BorrowedFdStream::from_raw_fd(fd) };
            return Ok(Box::new(KdcanSerialTransport::new(
                stream,
                mode,
                endpoint.to_string(),
            )));
        }
        #[cfg(not(unix))]
        {
            let _ = rest;
            return Err(HandosError::Connection {
                address: endpoint.to_string(),
                reason: "kdcan-fd:// transport is only supported on Unix/Android targets".to_string(),
            });
        }
    }

    Ok(Box::new(MhdWifiTransport::connect(
        endpoint,
        Duration::from_secs(5),
    )?))
}

struct CachedSession {
    host: String,
    backup_dir: String,
    session: HandosSession<Box<dyn Transport>>,
}

fn session_pool() -> &'static Mutex<Option<CachedSession>> {
    static POOL: OnceLock<Mutex<Option<CachedSession>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(None))
}

fn with_session<T, F>(host: &str, backup_dir: &str, mut op: F) -> crate::Result<T>
where
    F: FnMut(&mut HandosSession<Box<dyn Transport>>) -> crate::Result<T>,
{
    if host.starts_with("kdcan-fd://") {
        let transport = connect_transport(host)?;
        let backups = BackupManager::new(backup_dir)?;
        let mut session = HandosSession::new(transport, backups);
        return op(&mut session);
    }

    let mut guard = session_pool()
        .lock()
        .map_err(|_| HandosError::Protocol("session pool lock poisoned".to_string()))?;

    let matches = guard
        .as_ref()
        .map(|c| c.host == host && c.backup_dir == backup_dir)
        .unwrap_or(false);
    if !matches {
        *guard = None;
    }

    if let Some(cached) = guard.as_mut() {
        match op(&mut cached.session) {
            Ok(val) => return Ok(val),
            Err(HandosError::Io(_)) | Err(HandosError::Connection { .. }) => {
                // Drop stale socket and reconnect once below.
                *guard = None;
            }
            Err(other) => return Err(other),
        }
    }

    let transport = connect_transport(host)?;
    let backups = BackupManager::new(backup_dir)?;
    let mut session = HandosSession::new(transport, backups);
    let result = op(&mut session)?;
    *guard = Some(CachedSession {
        host: host.to_string(),
        backup_dir: backup_dir.to_string(),
        session,
    });
    Ok(result)
}

pub fn disconnect_session_json() -> String {
    let res = (|| -> crate::Result<bool> {
        let mut guard = session_pool()
            .lock()
            .map_err(|_| HandosError::Protocol("session pool lock poisoned".to_string()))?;
        let had_session = guard.is_some();
        *guard = None;
        Ok(had_session)
    })();
    to_json_response(res)
}

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
    let res = with_session(host, backup_dir, |session| session.scan_vehicle());
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
        with_session(host, backup_dir, |session| {
            if enable {
                session.apply_preset_safely(&preset)
            } else {
                session.revert_preset_safely(&preset)
            }
        })
    })();
    to_json_response(res)
}

pub fn preset_statuses_json(host: &str, backup_dir: &str) -> String {
    let res = with_session(host, backup_dir, |session| session.check_presets_status());
    to_json_response(res)
}

pub fn list_backups_json(vin: &str, backup_dir: &str) -> String {
    let res = (|| {
        let backups = BackupManager::new(backup_dir)?;
        backups.list_backups(vin)
    })();
    to_json_response(res)
}

pub fn restore_backup_json(
    host: &str,
    ecu_name: &str,
    backup_path: &str,
    backup_dir: &str,
) -> String {
    let res = with_session(host, backup_dir, |session| {
        let vehicle = session.scan_vehicle()?;
        let ecu = vehicle
            .ecus
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(ecu_name))
            .cloned()
            .ok_or_else(|| crate::HandosError::Protocol(format!("ECU '{ecu_name}' not found")))?;
        session.restore_backup(&ecu, backup_path)
    });
    to_json_response(res)
}

pub fn run_service_job_json(host: &str, job_id: &str, backup_dir: &str) -> String {
    let res = (|| {
        let job = match job_id {
            "register_battery" => crate::ServiceJob::RegisterBattery,
            "reset_oil_service" => crate::ServiceJob::ResetOilService,
            "reset_brake_service" => crate::ServiceJob::ResetBrakeService,
            other => {
                return Err(crate::HandosError::Protocol(format!(
                    "unknown service job '{other}'"
                )))
            }
        };
        with_session(host, backup_dir, |session| session.run_service_job(job))
    })();
    to_json_response(res)
}

pub fn read_telemetry_json(host: &str, backup_dir: &str) -> String {
    let res = with_session(host, backup_dir, |session| session.read_live_telemetry());
    to_json_response(res)
}

pub fn read_ecu_params_json(host: &str, ecu_name: &str, backup_dir: &str) -> String {
    let res = with_session(host, backup_dir, |session| session.read_ecu_params(ecu_name));
    to_json_response(res)
}

pub fn set_ecu_param_json(
    host: &str,
    ecu_name: &str,
    parameter: &str,
    value: &str,
    backup_dir: &str,
) -> String {
    let res = with_session(host, backup_dir, |session| {
        session.apply_single_param_safely(ecu_name, parameter, value)
    });
    to_json_response(res)
}

pub fn read_dtcs_json(host: &str, backup_dir: &str) -> String {
    let res = with_session(host, backup_dir, |session| {
        let info = session.scan_vehicle()?;
        let mut all = Vec::new();
        for ecu in &info.ecus {
            all.extend(session.read_dtcs(ecu)?);
        }
        Ok(all)
    });
    to_json_response(res)
}

pub fn clear_dtcs_json(host: &str, backup_dir: &str) -> String {
    let res = with_session(host, backup_dir, |session| {
        let info = session.scan_vehicle()?;
        for ecu in &info.ecus {
            session.clear_dtcs(ecu)?;
        }
        Ok(info.ecus.len())
    });
    to_json_response(res)
}

pub fn discover_mhd_endpoints_json(extra_candidates_csv: &str, timeout_ms: u64) -> String {
    let extra: Vec<String> = extra_candidates_csv
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let clamped_ms = timeout_ms.clamp(100, 10_000);
    let found = crate::transport::discover_mhd_endpoints(
        Duration::from_millis(clamped_ms),
        &extra,
    );
    to_json_response(Ok(found))
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
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeDisconnect(
        mut env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        env.new_string(disconnect_session_json())
            .unwrap()
            .into_raw()
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
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeDiscoverMhdEndpoints(
        mut env: JNIEnv,
        _class: JClass,
        extra_candidates_csv: JString,
        timeout_ms: jni::sys::jint,
    ) -> jstring {
        let extra = get_str(&mut env, extra_candidates_csv);
        let ms = timeout_ms.max(100) as u64;
        env.new_string(discover_mhd_endpoints_json(&extra, ms))
            .unwrap()
            .into_raw()
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

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeListBackups(
        mut env: JNIEnv,
        _class: JClass,
        vin: JString,
        backup_dir: JString,
    ) -> jstring {
        let v = get_str(&mut env, vin);
        let d = get_str(&mut env, backup_dir);
        env.new_string(list_backups_json(&v, &d))
            .unwrap()
            .into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeRestoreBackup(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        ecu_name: JString,
        backup_path: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let e = get_str(&mut env, ecu_name);
        let p = get_str(&mut env, backup_path);
        let d = get_str(&mut env, backup_dir);
        env.new_string(restore_backup_json(&h, &e, &p, &d))
            .unwrap()
            .into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeRunServiceJob(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        job_id: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let j = get_str(&mut env, job_id);
        let d = get_str(&mut env, backup_dir);
        env.new_string(run_service_job_json(&h, &j, &d))
            .unwrap()
            .into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeReadTelemetry(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let d = get_str(&mut env, backup_dir);
        env.new_string(read_telemetry_json(&h, &d))
            .unwrap()
            .into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeReadEcuParams(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        ecu_name: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let e = get_str(&mut env, ecu_name);
        let d = get_str(&mut env, backup_dir);
        env.new_string(read_ecu_params_json(&h, &e, &d))
            .unwrap()
            .into_raw()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_handos_bmw_HandosBridge_nativeSetEcuParam(
        mut env: JNIEnv,
        _class: JClass,
        host: JString,
        ecu_name: JString,
        parameter: JString,
        value: JString,
        backup_dir: JString,
    ) -> jstring {
        let h = get_str(&mut env, host);
        let e = get_str(&mut env, ecu_name);
        let p = get_str(&mut env, parameter);
        let v = get_str(&mut env, value);
        let d = get_str(&mut env, backup_dir);
        env.new_string(set_ecu_param_json(&h, &e, &p, &v, &d))
            .unwrap()
            .into_raw()
    }
}

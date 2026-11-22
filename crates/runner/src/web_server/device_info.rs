use super::*;

pub(crate) fn device_info_cache() -> &'static RwLock<Option<(Instant, serde_json::Value)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub(crate) fn get_device_info(identity_path: &Path) -> serde_json::Value {
    if let Ok(guard) = device_info_cache().read() {
        if let Some((cached_at, ref val)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return val.clone();
            }
        }
    }

    let Some(home) = crate::device_name::home_dir() else {
        return json!({
            "device_id": "unknown",
            "public_key": "",
            "paired": false,
            "gateway_url": "",
            "device_name": crate::device_name::fallback_device_name(),
        });
    };

    // The binding lives beside the identity, wherever the Runner was told to
    // keep it.
    let agenthub_dir = identity_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".agenthub"));
    let binding_path = agenthub_dir.join("binding.json");

    let mut device_id = "unknown".to_string();
    let mut public_key = "".to_string();

    if let Ok(identity) = crate::device::DeviceIdentity::load(identity_path) {
        device_id = identity.device_id().to_string();
        public_key = identity.public_key_hex();
    } else if let Ok(content) = std::fs::read_to_string(identity_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(id) = val.get("device_id").and_then(|v| v.as_str()) {
                device_id = id.to_string();
            }
            if let Some(pk) = val.get("public_key").and_then(|v| v.as_str()) {
                public_key = pk.to_string();
            }
        }
    }

    let mut paired = false;
    let mut gateway_url = String::new();
    let mut paired_at = String::new();
    if let Ok(binding_content) = std::fs::read_to_string(&binding_path) {
        if let Ok(binding_val) = serde_json::from_str::<serde_json::Value>(&binding_content) {
            paired = binding_val
                .get("registered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            gateway_url = binding_val
                .get("gateway")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            paired_at = binding_val
                .get("paired_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
    }

    let res = json!({
        "device_id": device_id,
        "public_key": public_key,
        "paired": paired,
        "gateway_url": gateway_url,
        "paired_at": paired_at,
        "device_name": crate::device_name::resolve(None, Some(identity_path)),
    });

    if let Ok(mut guard) = device_info_cache().write() {
        *guard = Some((Instant::now(), res.clone()));
    }
    res
}

pub(crate) fn cached_agent_installations() -> [(bool, Option<String>); 5] {
    let home = crate::collectors::home_dir();
    [
        find_executable(
            "claude",
            &[r"AppData\Roaming\npm\claude.cmd", r".local\bin\claude"],
        ),
        find_executable(
            "codex",
            &[r"AppData\Roaming\npm\codex.cmd", r".local\bin\codex"],
        ),
        check_agent_dir("AppData\\Local\\AIClaude"),
        crate::collectors::antigravity::check_antigravity_installed_at(home.as_deref()),
        (false, None),
    ]
}

pub(crate) fn executable_cache()
-> &'static RwLock<HashMap<String, (Instant, (bool, Option<String>))>> {
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn find_executable(name: &str, relative_hints: &[&str]) -> (bool, Option<String>) {
    if let Ok(guard) = executable_cache().read() {
        if let Some((cached_at, res)) = guard.get(name) {
            if cached_at.elapsed() < Duration::from_secs(30) {
                return res.clone();
            }
        }
    }

    let mut found = (false, None);
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home_path = PathBuf::from(home);
        for hint in relative_hints {
            let full = home_path.join(hint);
            if full.exists() {
                found = (true, Some(full.to_string_lossy().to_string()));
                break;
            }
        }
    }

    if !found.0 {
        if let Some(path_var) = std::env::var_os("PATH") {
            let paths = std::env::split_paths(&path_var);
            #[cfg(target_os = "windows")]
            let exts = [".cmd", ".exe", ".bat", ""];
            #[cfg(not(target_os = "windows"))]
            let exts = [""];

            for p in paths {
                for ext in &exts {
                    let candidate = p.join(format!("{}{}", name, ext));
                    if candidate.is_file() {
                        found = (true, Some(candidate.to_string_lossy().to_string()));
                        break;
                    }
                }
                if found.0 {
                    break;
                }
            }
        }
    }

    if let Ok(mut guard) = executable_cache().write() {
        guard.insert(name.to_string(), (Instant::now(), found.clone()));
    }
    found
}

pub(crate) fn agent_dir_cache()
-> &'static RwLock<HashMap<String, (Instant, (bool, Option<String>))>> {
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn check_agent_dir(dir_name: &str) -> (bool, Option<String>) {
    if let Ok(guard) = agent_dir_cache().read() {
        if let Some((cached_at, res)) = guard.get(dir_name) {
            if cached_at.elapsed() < Duration::from_secs(30) {
                return res.clone();
            }
        }
    }

    let mut found = (false, None);
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let full = PathBuf::from(home).join(dir_name);
        if full.is_dir() {
            found = (true, Some(full.to_string_lossy().to_string()));
        }
    }

    if let Ok(mut guard) = agent_dir_cache().write() {
        guard.insert(dir_name.to_string(), (Instant::now(), found.clone()));
    }
    found
}

pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

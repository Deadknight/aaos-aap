//! Small, dependency-free loader for ARDA's documented `config.toml` profile.
//!
//! The file is valid TOML, but this parser intentionally accepts only the
//! simple section/key/value subset used by ARDA. Unknown keys fail loudly so a
//! typo cannot silently change a vehicle/session test.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
enum ConfigValue {
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl ConfigValue {
    fn as_string(&self, key: &str) -> Result<String, Box<dyn std::error::Error>> {
        match self {
            Self::String(value) => Ok(value.clone()),
            Self::Integer(value) => Ok(value.to_string()),
            Self::Float(value) => Ok(value.to_string()),
            Self::Bool(_) => Err(format!("config key {key} requires a string/number, got boolean").into()),
        }
    }

    fn as_bool(&self, key: &str) -> Result<bool, Box<dyn std::error::Error>> {
        match self {
            Self::Bool(value) => Ok(*value),
            _ => Err(format!("config key {key} requires true or false").into()),
        }
    }
}

#[derive(Debug)]
pub struct ExpandedArgs {
    pub argv: Vec<String>,
    pub config_path: Option<PathBuf>,
}

pub fn expand_aap_session_args(argv: Vec<String>) -> Result<ExpandedArgs, Box<dyn std::error::Error>> {
    let mut explicit = Vec::new();
    let mut config_path: Option<PathBuf> = None;
    let mut i = 0usize;

    while i < argv.len() {
        match argv[i].as_str() {
            "--config" | "--config-file" => {
                i += 1;
                let path = argv.get(i).ok_or("--config requires a path")?;
                if config_path.is_some() {
                    return Err("only one --config may be supplied".into());
                }
                config_path = Some(PathBuf::from(path));
            }
            other => explicit.push(other.to_string()),
        }
        i += 1;
    }

    let mut expanded = Vec::new();
    if let Some(path) = config_path.as_ref() {
        expanded.extend(load_aap_session_config(path)?);
    }
    // CLI is deliberately appended after config-derived args. The existing
    // parser is left-to-right, so an explicit CLI flag overrides config.toml.
    expanded.extend(explicit);

    Ok(ExpandedArgs {
        argv: expanded,
        config_path,
    })
}



pub fn expand_bt_bootstrap_args(argv: Vec<String>) -> Result<ExpandedArgs, Box<dyn std::error::Error>> {
    let mut explicit = Vec::new();
    let mut config_path: Option<PathBuf> = None;
    let mut i = 0usize;

    while i < argv.len() {
        match argv[i].as_str() {
            "--config" | "--config-file" => {
                i += 1;
                let path = argv.get(i).ok_or("--config requires a path")?;
                if config_path.is_some() {
                    return Err("only one --config may be supplied".into());
                }
                config_path = Some(PathBuf::from(path));
            }
            other => explicit.push(other.to_string()),
        }
        i += 1;
    }

    let mut expanded = Vec::new();
    if let Some(path) = config_path.as_ref() {
        expanded.extend(load_bt_bootstrap_config(path)?);
    }
    // Explicit CLI values intentionally come last and therefore override TOML.
    expanded.extend(explicit);

    Ok(ExpandedArgs {
        argv: expanded,
        config_path,
    })
}

fn load_bt_bootstrap_config(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("cannot read config {}: {err}", path.display()))?;
    let mut values = parse_simple_toml(&text)?;

    // `rn3b.*` is retained as a read-only compatibility alias for configs
    // generated before the runtime service was renamed to arda-connectivityd.
    for suffix in [
        "transport",
        "addr",
        "socket",
        "bridge_token",
        "bridge_token_file",
        "once",
        "keep_bootstrap_rfcomm_open",
        "keep_bluetooth_profiles_connected",
    ] {
        let legacy = format!("rn3b.{suffix}");
        let canonical = format!("connectivity.{suffix}");
        if let Some(value) = values.remove(&legacy) {
            if values.insert(canonical.clone(), value).is_some() {
                return Err(format!(
                    "config contains both legacy {legacy} and canonical {canonical}"
                )
                .into());
            }
        }
    }

    let supported: HashSet<&'static str> = [
        "connectivity.transport",
        "connectivity.addr",
        "connectivity.socket",
        "connectivity.bridge_token",
        "connectivity.bridge_token_file",
        "connectivity.once",
        "connectivity.keep_bootstrap_rfcomm_open",
        "connectivity.keep_bluetooth_profiles_connected",
        "aap_key_update.enabled",
        "aap_key_update.control_url",
        "aap_key_update.allow_direct_internet",
        "aap_key_update.check_on_start",
        "aap_key_update.check_on_direct_ready",
        "aap_key_update.check_on_proxy_ready",
        "aap_key_update.check_interval_seconds",
        "aap_key_update.connect_debounce_seconds",
        "aap_key_update.retry_interval_seconds",
        "aap_key_update.download_timeout_seconds",
        "aap_key_update.max_manifest_bytes",
        "aap_key_update.max_archive_bytes",
    ]
    .into_iter()
    .collect();

    for key in values.keys() {
        if !supported.contains(key.as_str()) {
            return Err(format!(
                "unknown connectivity config key {key} in {}",
                path.display()
            )
            .into());
        }
    }

    if values.contains_key("connectivity.bridge_token")
        && values.contains_key("connectivity.bridge_token_file")
    {
        return Err(
            "use only one of connectivity.bridge_token or connectivity.bridge_token_file".into(),
        );
    }

    let mut out = Vec::new();
    push_value(
        &values,
        "connectivity.transport",
        "--transport",
        &mut out,
    )?;
    push_value(&values, "connectivity.addr", "--addr", &mut out)?;
    push_value(&values, "connectivity.socket", "--socket", &mut out)?;
    push_value(
        &values,
        "connectivity.bridge_token",
        "--bridge-token",
        &mut out,
    )?;
    push_value(
        &values,
        "connectivity.bridge_token_file",
        "--bridge-token-file",
        &mut out,
    )?;
    push_bool(
        &values,
        "connectivity.once",
        "--once",
        "--no-once",
        &mut out,
    )?;
    push_bool(
        &values,
        "connectivity.keep_bootstrap_rfcomm_open",
        "--keep-bootstrap-rfcomm-open",
        "--no-keep-bootstrap-rfcomm-open",
        &mut out,
    )?;
    push_bool(
        &values,
        "connectivity.keep_bluetooth_profiles_connected",
        "--keep-bluetooth-profiles-connected",
        "--no-keep-bluetooth-profiles-connected",
        &mut out,
    )?;

    push_bool(
        &values,
        "aap_key_update.enabled",
        "--aap-key-update",
        "--no-aap-key-update",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.control_url",
        "--aap-key-control-url",
        &mut out,
    )?;
    push_bool(
        &values,
        "aap_key_update.allow_direct_internet",
        "--aap-key-direct-internet",
        "--no-aap-key-direct-internet",
        &mut out,
    )?;
    push_bool(
        &values,
        "aap_key_update.check_on_start",
        "--aap-key-check-on-start",
        "--no-aap-key-check-on-start",
        &mut out,
    )?;
    push_bool(
        &values,
        "aap_key_update.check_on_direct_ready",
        "--aap-key-check-on-direct-ready",
        "--no-aap-key-check-on-direct-ready",
        &mut out,
    )?;
    push_bool(
        &values,
        "aap_key_update.check_on_proxy_ready",
        "--aap-key-check-on-proxy-ready",
        "--no-aap-key-check-on-proxy-ready",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.check_interval_seconds",
        "--aap-key-check-interval-seconds",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.connect_debounce_seconds",
        "--aap-key-connect-debounce-seconds",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.retry_interval_seconds",
        "--aap-key-retry-interval-seconds",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.download_timeout_seconds",
        "--aap-key-download-timeout-seconds",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.max_manifest_bytes",
        "--aap-key-max-manifest-bytes",
        &mut out,
    )?;
    push_value(
        &values,
        "aap_key_update.max_archive_bytes",
        "--aap-key-max-archive-bytes",
        &mut out,
    )?;

    Ok(out)
}

pub fn render_resolved_args(argv: Vec<String>) -> Result<String, Box<dyn std::error::Error>> {
    let expanded = expand_aap_session_args(argv)?;
    let mut out = String::new();
    if let Some(path) = expanded.config_path {
        out.push_str(&format!("config={}\n", path.display()));
    } else {
        out.push_str("config=none\n");
    }
    out.push_str("resolved argv:\n");
    for arg in expanded.argv {
        out.push_str("  ");
        out.push_str(&shell_display(&arg));
        out.push('\n');
    }
    Ok(out)
}

fn load_aap_session_config(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("cannot read config {}: {err}", path.display()))?;
    let values = parse_simple_toml(&text)?;

    let supported: HashSet<&'static str> = [
        "transport.mode",
        "transport.usb_path",
        "transport.tcp_addr",
        "session.keys",
        "session.dump",
        "session.response",
        "session.max_frames",
        "session.timeout_ms",
        "session.host_liveness_timeout_ms",
        "session.max_bytes",
        "session.quiet_ping",
        "session.log_level",
        "session.usb_reset_on_exit",
        "video.native",
        "video.auto_config",
        "video.config_policy",
        "video.bit_rate",
        "video.frames",
        "video.apply_wm_config",
        "video.wm_size_mode",
        "video.wm_density_mode",
        "video.wm_settle_ms",
        "video.fast_start",
        "video.display_profile",
        "video.first_frame_kick",
        "input.open",
        "input.key_binding",
        "input.inject",
        "input.inject_mode",
        "input.app_move_min_ms",
        "input.key_inject",
        "audio.open",
        "audio.stream",
        "sensor.open",
        "sensor.request_all",
        "sensor.max_type",
        "sensor.period_ms",
        "sensor.jsonl",
        "sensor.forward_os",
        "sensor.forward_vhal",
        "sensor.location_provider",
        "sensor.location_min_ms",
        "sensor.vhal_min_ms",
        "sensor.wasm",
        "sensor.wasm_fuel",
        "sensor.wasm_max_commands",
    ]
    .into_iter()
    .collect();

    for key in values.keys() {
        if !supported.contains(key.as_str()) {
            return Err(format!("unknown config key {key} in {}", path.display()).into());
        }
    }

    let mut out = Vec::new();

    push_value(&values, "transport.mode", "--transport", &mut out)?;
    push_value(&values, "transport.usb_path", "--usb-path", &mut out)?;
    push_value(&values, "transport.tcp_addr", "--addr", &mut out)?;

    push_value(&values, "session.keys", "--keys", &mut out)?;
    if let Some(value) = values.get("session.dump") {
        let value = value.as_string("session.dump")?;
        if matches!(value.as_str(), "none" | "-" | "off") {
            out.push("--no-dump".to_string());
        } else {
            out.push("--dump".to_string());
            out.push(value);
        }
    }
    push_value(&values, "session.response", "--response", &mut out)?;
    push_value(&values, "session.max_frames", "--max-frames", &mut out)?;
    push_value(&values, "session.timeout_ms", "--timeout-ms", &mut out)?;
    push_value(
        &values,
        "session.host_liveness_timeout_ms",
        "--host-liveness-timeout-ms",
        &mut out,
    )?;
    push_value(&values, "session.max_bytes", "--max-bytes", &mut out)?;
    push_bool(&values, "session.quiet_ping", "--quiet-ping", "--log-ping", &mut out)?;
    push_value(&values, "session.log_level", "--log-level", &mut out)?;
    push_bool(
        &values,
        "session.usb_reset_on_exit",
        "--usb-reset-on-exit",
        "--no-usb-reset-on-exit",
        &mut out,
    )?;

    push_bool_one_way(&values, "video.native", "--video-native", &mut out)?;
    push_bool(&values, "video.auto_config", "--video-auto-config", "--no-video-auto-config", &mut out)?;
    push_value(&values, "video.config_policy", "--video-config-policy", &mut out)?;
    push_value(&values, "video.bit_rate", "--video-bit-rate", &mut out)?;
    push_value(&values, "video.frames", "--video-frames", &mut out)?;
    push_bool(&values, "video.apply_wm_config", "--apply-wm-config", "--no-apply-wm-config", &mut out)?;
    push_value(&values, "video.wm_size_mode", "--wm-size-mode", &mut out)?;
    push_value(&values, "video.wm_density_mode", "--wm-density-mode", &mut out)?;
    push_value(&values, "video.wm_settle_ms", "--wm-settle-ms", &mut out)?;
    push_bool(&values, "video.fast_start", "--fast-start", "--no-fast-start", &mut out)?;
    push_value(&values, "video.display_profile", "--display-profile", &mut out)?;
    push_bool(&values, "video.first_frame_kick", "--video-first-frame-kick", "--no-video-first-frame-kick", &mut out)?;

    push_bool(&values, "input.open", "--input-open", "--no-input-open", &mut out)?;
    push_bool(&values, "input.key_binding", "--input-key-binding", "--no-input-key-binding", &mut out)?;
    push_bool(&values, "input.inject", "--input-inject", "--no-input-inject", &mut out)?;
    push_value(&values, "input.inject_mode", "--input-inject-mode", &mut out)?;
    push_value(&values, "input.app_move_min_ms", "--input-app-move-min-ms", &mut out)?;
    push_bool(&values, "input.key_inject", "--input-key-inject", "--no-input-key-inject", &mut out)?;

    push_bool(&values, "audio.open", "--audio-open", "--no-audio-open", &mut out)?;
    push_value(&values, "audio.stream", "--audio-stream", &mut out)?;

    push_bool(&values, "sensor.open", "--sensor-open", "--no-sensor-open", &mut out)?;
    push_bool(&values, "sensor.request_all", "--sensor-request-all", "--no-sensor-request-all", &mut out)?;
    push_value(&values, "sensor.max_type", "--sensor-max-type", &mut out)?;
    push_value(&values, "sensor.period_ms", "--sensor-period-ms", &mut out)?;
    if let Some(value) = values.get("sensor.jsonl") {
        let value = value.as_string("sensor.jsonl")?;
        if matches!(value.as_str(), "none" | "-" | "off") {
            out.push("--no-sensor-jsonl".to_string());
        } else {
            out.push("--sensor-jsonl".to_string());
            out.push(value);
        }
    }
    // The current OS forwarding path is LOCATION-only. Use the dedicated
    // location flags so enabling it never implicitly enables VHAL forwarding.
    push_bool(
        &values,
        "sensor.forward_os",
        "--sensor-forward-location",
        "--no-sensor-forward-location",
        &mut out,
    )?;
    push_bool(&values, "sensor.forward_vhal", "--sensor-forward-vhal", "--no-sensor-forward-vhal", &mut out)?;
    push_value(&values, "sensor.location_provider", "--sensor-location-provider", &mut out)?;
    push_value(&values, "sensor.location_min_ms", "--sensor-location-min-ms", &mut out)?;
    push_value(&values, "sensor.vhal_min_ms", "--sensor-vhal-min-ms", &mut out)?;
    push_value(&values, "sensor.wasm", "--sensor-wasm", &mut out)?;
    push_value(&values, "sensor.wasm_fuel", "--sensor-wasm-fuel", &mut out)?;
    push_value(&values, "sensor.wasm_max_commands", "--sensor-wasm-max-commands", &mut out)?;

    Ok(out)
}

fn push_value(
    values: &BTreeMap<String, ConfigValue>,
    key: &str,
    flag: &str,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(value) = values.get(key) {
        out.push(flag.to_string());
        out.push(value.as_string(key)?);
    }
    Ok(())
}

fn push_bool(
    values: &BTreeMap<String, ConfigValue>,
    key: &str,
    true_flag: &str,
    false_flag: &str,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(value) = values.get(key) {
        out.push(if value.as_bool(key)? { true_flag } else { false_flag }.to_string());
    }
    Ok(())
}

fn push_bool_one_way(
    values: &BTreeMap<String, ConfigValue>,
    key: &str,
    true_flag: &str,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(value) = values.get(key) {
        if value.as_bool(key)? {
            out.push(true_flag.to_string());
        }
    }
    Ok(())
}

fn parse_simple_toml(text: &str) -> Result<BTreeMap<String, ConfigValue>, Box<dyn std::error::Error>> {
    let mut section = String::new();
    let mut out = BTreeMap::new();

    for (index, original) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(original).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.starts_with("[[") {
                return Err(format!("config line {line_no}: only simple [section] headers are supported").into());
            }
            section = line[1..line.len() - 1].trim().to_string();
            if section.is_empty() {
                return Err(format!("config line {line_no}: empty section name").into());
            }
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(format!("config line {line_no}: expected key = value").into());
        };
        let key = raw_key.trim();
        if key.is_empty() {
            return Err(format!("config line {line_no}: empty key").into());
        }
        let full_key = if section.is_empty() {
            key.to_string()
        } else {
            format!("{section}.{key}")
        };
        if out.contains_key(&full_key) {
            return Err(format!("config line {line_no}: duplicate key {full_key}").into());
        }
        out.insert(full_key, parse_value(raw_value.trim(), line_no)?);
    }

    Ok(out)
}

fn parse_value(raw: &str, line_no: usize) -> Result<ConfigValue, Box<dyn std::error::Error>> {
    if raw.starts_with('"') {
        if !raw.ends_with('"') || raw.len() < 2 {
            return Err(format!("config line {line_no}: unterminated string").into());
        }
        let inner = &raw[1..raw.len() - 1];
        return Ok(ConfigValue::String(unescape_basic_string(inner, line_no)?));
    }
    if raw.starts_with('\'') {
        if !raw.ends_with('\'') || raw.len() < 2 {
            return Err(format!("config line {line_no}: unterminated literal string").into());
        }
        return Ok(ConfigValue::String(raw[1..raw.len() - 1].to_string()));
    }
    match raw {
        "true" => return Ok(ConfigValue::Bool(true)),
        "false" => return Ok(ConfigValue::Bool(false)),
        _ => {}
    }

    let normalized = raw.replace('_', "");
    if let Ok(value) = normalized.parse::<i64>() {
        return Ok(ConfigValue::Integer(value));
    }
    if let Ok(value) = normalized.parse::<f64>() {
        return Ok(ConfigValue::Float(value));
    }
    Err(format!("config line {line_no}: unsupported value {raw:?}").into())
}

fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if quote == Some('"') && ch == '\\' {
            out.push(ch);
            escaped = true;
            continue;
        }
        if ch == '"' || ch == '\'' {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            out.push(ch);
            continue;
        }
        if ch == '#' && quote.is_none() {
            break;
        }
        out.push(ch);
    }
    out
}

fn unescape_basic_string(value: &str, line_no: usize) -> Result<String, Box<dyn std::error::Error>> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            return Err(format!("config line {line_no}: dangling string escape").into());
        };
        match next {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => return Err(format!("config line {line_no}: unsupported escape \\{other}").into()),
        }
    }
    Ok(out)
}

fn shell_display(value: &str) -> String {
    if value.chars().all(|ch| ch.is_ascii_alphanumeric() || "-._/:".contains(ch)) {
        value.to_string()
    } else {
        format!("'{value}'")
    }
}

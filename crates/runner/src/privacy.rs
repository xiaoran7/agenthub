use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::ProjectRef;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum PrivacyError {
    #[error("fingerprint key cannot be empty")]
    EmptyFingerprintKey,
}

#[derive(Clone)]
pub struct ProjectRedactor {
    fingerprint_key: Vec<u8>,
}

impl ProjectRedactor {
    pub fn new(fingerprint_key: impl AsRef<[u8]>) -> Result<Self, PrivacyError> {
        let fingerprint_key = fingerprint_key.as_ref();
        if fingerprint_key.is_empty() {
            return Err(PrivacyError::EmptyFingerprintKey);
        }
        Ok(Self {
            fingerprint_key: fingerprint_key.to_vec(),
        })
    }

    pub fn redact(&self, raw_cwd: &str) -> ProjectRef {
        let normalized = normalize_path_for_fingerprint(raw_cwd);
        let display_name = project_display_name(raw_cwd);
        let mut mac =
            HmacSha256::new_from_slice(&self.fingerprint_key).expect("HMAC accepts any key length");
        mac.update(normalized.as_bytes());

        ProjectRef {
            display_name,
            fingerprint: hex::encode(mac.finalize().into_bytes()),
        }
    }
}

pub fn workspace_id_for_path(raw_cwd: &str) -> String {
    let normalized = normalize_path_for_fingerprint(raw_cwd);
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"agenthub_workspace_v1:");
    hasher.update(normalized.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let display = project_display_name(raw_cwd);
    let safe_display: String = display
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(24)
        .collect();
    format!("ws-{safe_display}-{}", &hash[..16])
}

fn normalize_path_for_fingerprint(raw_cwd: &str) -> String {
    let mut normalized = raw_cwd
        .trim()
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/");
    if normalized.starts_with("//?/") {
        normalized = normalized[4..].to_string();
    }
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        normalized = normalized.to_lowercase();
    } else {
        #[cfg(windows)]
        {
            normalized = normalized.to_lowercase();
        }
    }
    normalized
}

pub fn project_display_name(raw_cwd: &str) -> String {
    let trimmed = raw_cwd.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return "unknown-project".into();
    }
    // A session started directly in the home directory would otherwise use the
    // account name as its display name; the username is not project metadata.
    if is_current_user_home(trimmed) {
        return "home".into();
    }

    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "unknown-project".into())
}

/// Builds a human-readable, username-free display path for a workspace
/// (e.g. `~/Desktop`, `D: ClaudeSpace/Project/Agent-Nexus`, `www/workspace/app`)
/// that never exposes raw OS home usernames or triggers absolute-path redaction.
pub fn workspace_display_path(raw_cwd: &str) -> String {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|v| v.to_string_lossy().into_owned());
    workspace_display_path_with_home(raw_cwd, home.as_deref())
}

fn workspace_display_path_with_home(raw_cwd: &str, current_home: Option<&str>) -> String {
    let mut cleaned = raw_cwd.trim().replace('\\', "/");
    if let Some(rest) = cleaned.strip_prefix("//?/") {
        cleaned = rest.to_owned();
    }
    let cleaned = cleaned.trim_end_matches('/');
    if cleaned.is_empty() {
        return project_display_name(raw_cwd);
    }

    let formatted = if let Some(home) = current_home {
        let norm_home = home
            .trim()
            .replace('\\', "/")
            .trim_start_matches("//?/")
            .trim_end_matches('/')
            .to_owned();
        if !norm_home.is_empty() {
            let cwd_lower = cleaned.to_lowercase();
            let home_lower = norm_home.to_lowercase();
            if cwd_lower == home_lower {
                "~".to_owned()
            } else if cwd_lower.starts_with(&format!("{home_lower}/")) {
                format!("~/{}", &cleaned[norm_home.len() + 1..])
            } else {
                format_non_home_display_path(cleaned)
            }
        } else {
            format_non_home_display_path(cleaned)
        }
    } else {
        format_non_home_display_path(cleaned)
    };

    let compacted = compact_display_path(&formatted, 120);
    if compacted.is_empty()
        || is_absolute_filesystem_path(&compacted)
        || contains_absolute_filesystem_path(&compacted)
    {
        project_display_name(raw_cwd)
    } else {
        compacted
    }
}

fn format_non_home_display_path(cleaned: &str) -> String {
    let segments: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return "root".to_owned();
    }
    let bytes = cleaned.as_bytes();
    // Collapse generic Windows `C:/Users/<name>/...` to `~/...` so usernames stay private.
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_uppercase();
        if segments.len() >= 3 && segments[1].eq_ignore_ascii_case("users") {
            let tail = &segments[3..];
            return if tail.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", tail.join("/"))
            };
        }
        let tail = &segments[1..];
        return if tail.is_empty() {
            format!("{drive}:")
        } else {
            format!("{drive}: {}", tail.join("/"))
        };
    }
    // Collapse generic Unix `/Users/<name>/...`, `/home/<name>/...`, `/root/...`.
    if cleaned.starts_with('/') {
        if segments.len() >= 2
            && (segments[0].eq_ignore_ascii_case("users") || segments[0] == "home")
        {
            let tail = &segments[2..];
            return if tail.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", tail.join("/"))
            };
        }
        if segments[0] == "root" {
            let tail = &segments[1..];
            return if tail.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", tail.join("/"))
            };
        }
        return segments.join("/");
    }
    segments.join("/")
}

fn compact_display_path(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let (prefix, body) = if let Some(rest) = value.strip_prefix("~/") {
        ("~/…/", rest)
    } else if value.len() >= 3
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':'
        && value.as_bytes()[2] == b' '
    {
        (&value[..3], &value[3..])
    } else {
        ("…/", value)
    };
    let parts: Vec<&str> = body.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return value.chars().take(max_chars).collect();
    }
    let tail = parts[parts.len().saturating_sub(3)..].join("/");
    let candidate = if prefix.ends_with("…/") {
        format!("{prefix}{tail}")
    } else {
        format!("{prefix}…/{tail}")
    };
    if candidate.chars().count() <= max_chars {
        candidate
    } else {
        let leaf = parts.last().copied().unwrap_or(value);
        format!("…/{leaf}").chars().take(max_chars).collect()
    }
}

fn is_single_level_unix_root(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "/etc"
            | "/tmp"
            | "/var"
            | "/opt"
            | "/usr"
            | "/home"
            | "/root"
            | "/app"
            | "/data"
            | "/mnt"
            | "/srv"
            | "/bin"
            | "/lib"
            | "/sbin"
            | "/dev"
            | "/proc"
            | "/sys"
            | "/Users"
            | "/Volumes"
            | "/Applications"
    )
}

pub fn is_absolute_filesystem_path(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.starts_with("ws-") || trimmed.is_empty() {
        return false;
    }
    if let Some(rest) = trimmed
        .strip_prefix("file://")
        .or_else(|| trimmed.strip_prefix("FILE://"))
    {
        return rest.starts_with('/') || is_absolute_filesystem_path(rest);
    }
    if trimmed.contains("://") {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return true;
    }
    if trimmed.starts_with('/')
        && (trimmed[1..].contains('/') || is_single_level_unix_root(trimmed))
    {
        return true;
    }
    false
}

/// Recursively traverses a JSON Value and enforces that no absolute physical filesystem paths
/// leak to the remote Gateway. Any string matching an absolute filesystem path or explicit path field
/// is securely transformed into an irreversible workspace_id.
pub fn sanitize_workspace_paths_in_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let normalized_key = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                let is_sensitive_key = normalized_key == "env"
                    || normalized_key.contains("environment")
                    || normalized_key.contains("password")
                    || normalized_key.contains("passwd")
                    || normalized_key.contains("cookie")
                    || normalized_key.contains("authorization")
                    || normalized_key.contains("apikey")
                    || normalized_key == "token"
                    || normalized_key.ends_with("token")
                    || normalized_key.contains("accesstoken")
                    || normalized_key.contains("refreshtoken")
                    || normalized_key.contains("privkey")
                    || normalized_key.contains("privatekey")
                    || normalized_key == "secret"
                    || normalized_key.ends_with("secret");
                if is_sensitive_key {
                    *val = serde_json::Value::String("[redacted]".into());
                    continue;
                }
                let is_path_key = key == "workspace_path"
                    || key == "cwd"
                    || key == "workdir"
                    || key == "working_dir"
                    || key == "working_directory"
                    || key == "project_path"
                    || key == "root_path"
                    || key == "path"
                    || key == "file_path"
                    || key == "filePath";
                if let Some(path_str) = val.as_str() {
                    if (is_path_key || is_absolute_filesystem_path(path_str))
                        && !path_str.starts_with("ws-")
                        && !path_str.trim().is_empty()
                    {
                        *val = serde_json::Value::String(workspace_id_for_path(path_str));
                    } else if contains_absolute_filesystem_path(path_str) {
                        *val = serde_json::Value::String(redact_paths_inside_text(path_str));
                    }
                }
                sanitize_workspace_paths_in_json(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(path_str) = item.as_str() {
                    if is_absolute_filesystem_path(path_str) && !path_str.starts_with("ws-") {
                        *item = serde_json::Value::String(workspace_id_for_path(path_str));
                    } else if contains_absolute_filesystem_path(path_str) {
                        *item = serde_json::Value::String(redact_paths_inside_text(path_str));
                    }
                } else {
                    sanitize_workspace_paths_in_json(item);
                }
            }
        }
        _ => {}
    }
}

/// Rewrites each absolute path embedded in free text as `[local_path_redacted]`
/// while leaving the rest of the sentence intact.
///
/// Used for provider error prose, where the whole-string replacement would
/// destroy the explanation the remote console needs: a message such as
/// "model X unsupported; see C:\...\config" would otherwise arrive with no cause
/// at all. The path must be a whole whitespace- or delimiter-bounded token, so
/// ordinary prose and URL-like fragments are never mistaken for one.
pub fn redact_paths_inside_text(value: &str) -> String {
    if !contains_absolute_filesystem_path(value) {
        return value.to_owned();
    }
    let mut redacted = String::with_capacity(value.len());
    let mut chars = value.char_indices().peekable();
    let mut token = String::new();
    while let Some((idx, character)) = chars.next() {
        if (character == '"' || character == '\'' || character == '`')
            && !value[idx + 1..].is_empty()
        {
            // Check whether a matching closing quote frames an absolute path (including spaces).
            if let Some(close_rel) = value[idx + character.len_utf8()..].find(character) {
                let inner_start = idx + character.len_utf8();
                let inner_end = inner_start + close_rel;
                let inner = &value[inner_start..inner_end];
                if !inner.is_empty() && !inner.contains('\n') && is_absolute_filesystem_path(inner)
                {
                    push_token(&mut redacted, &mut token);
                    redacted.push(character);
                    redacted.push_str("[local_path_redacted]");
                    redacted.push(character);
                    while let Some(&(next_idx, _)) = chars.peek() {
                        chars.next();
                        if next_idx == inner_end {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        if character.is_whitespace() {
            push_token(&mut redacted, &mut token);
            redacted.push(character);
        } else {
            token.push(character);
        }
    }
    push_token(&mut redacted, &mut token);
    redacted
}

/// Emits one delimited token, stripping surrounding punctuation so a path inside
/// a sentence is still recognised as a path.
fn push_token(out: &mut String, token: &mut String) {
    if token.is_empty() {
        return;
    }
    if let Some((lhs, rhs)) = token.split_once('=')
        && !lhs.contains("://")
    {
        let mut rhs_buf = rhs.to_string();
        let mut lhs_out = String::new();
        out.push_str(lhs);
        out.push('=');
        push_token(&mut lhs_out, &mut rhs_buf);
        out.push_str(&lhs_out);
        token.clear();
        return;
    }
    let leading = token
        .chars()
        .take_while(|c| matches!(c, '(' | '[' | '{' | '"' | '\'' | '`'))
        .count();
    let trailing = token
        .chars()
        .rev()
        .take_while(|c| {
            matches!(
                c,
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'' | '`'
            )
        })
        .count();
    let leading = leading.min(token.len());
    let trailing = trailing.min(token.len() - leading);
    let (prefix, rest) = token.split_at(leading);
    let (body, suffix) = rest.split_at(rest.len() - trailing);
    if is_absolute_filesystem_path(body) {
        out.push_str(prefix);
        out.push_str("[local_path_redacted]");
        out.push_str(suffix);
    } else {
        out.push_str(token);
    }
    token.clear();
}

fn contains_absolute_filesystem_path(value: &str) -> bool {
    if value
        .split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}'
                )
        })
        .map(|token| {
            let trimmed = token.trim_start_matches([':', '=', ',', ';']);
            trimmed.split_once('=').map_or(trimmed, |(_, rhs)| rhs)
        })
        .any(is_absolute_filesystem_path)
    {
        return true;
    }
    for quote in ['"', '\'', '`'] {
        let mut rest = value;
        while let Some(start) = rest.find(quote) {
            let after = &rest[start + 1..];
            let Some(end) = after.find(quote) else {
                break;
            };
            let inner = &after[..end];
            if !inner.contains('\n') && is_absolute_filesystem_path(inner) {
                return true;
            }
            rest = &after[end + 1..];
        }
    }
    false
}

fn is_current_user_home(trimmed_cwd: &str) -> bool {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    home.is_some_and(|home| matches_home(trimmed_cwd, home.to_string_lossy().as_ref()))
}

fn matches_home(trimmed_cwd: &str, home: &str) -> bool {
    let home = home.trim().trim_end_matches(['/', '\\']);
    !home.is_empty()
        && normalize_path_for_fingerprint(trimmed_cwd) == normalize_path_for_fingerprint(home)
}

#[cfg(test)]
#[path = "privacy_tests.rs"]
mod tests;

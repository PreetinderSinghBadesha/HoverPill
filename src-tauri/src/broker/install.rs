//! Adds HoverPill's `PermissionRequest` hook to Claude Code's user settings
//! (`~/.claude/settings.json`), or removes it. Everything else in the file is left as is.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{PATH, PORT};

fn hook_url() -> String {
    format!("http://127.0.0.1:{PORT}{PATH}")
}

/// Recognises our entry by URL prefix, so a changed path or token is still replaced.
fn is_ours(hook: &Value) -> bool {
    hook.get("type").and_then(Value::as_str) == Some("http")
        && hook
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.starts_with(&format!("http://127.0.0.1:{PORT}/claude/")))
}

/// Removes our hook from every `PermissionRequest` matcher group, then drops whatever that
/// leaves empty.
pub fn without_hook(mut settings: Value) -> Value {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return settings;
    };
    if let Some(groups) = hooks.get_mut("PermissionRequest").and_then(Value::as_array_mut) {
        for group in groups.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|h| !is_ours(h));
            }
        }
        groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|l| !l.is_empty()));
        if groups.is_empty() {
            hooks.remove("PermissionRequest");
        }
    }
    if hooks.is_empty() {
        settings.as_object_mut().map(|o| o.remove("hooks"));
    }
    settings
}

pub fn with_hook(settings: Value, token: &str) -> Value {
    let mut settings = without_hook(settings);
    let root = settings.as_object_mut().expect("settings root is an object");
    let hooks = root.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let groups = hooks
        .as_object_mut()
        .expect("`hooks` is an object")
        .entry("PermissionRequest")
        .or_insert_with(|| json!([]));
    groups.as_array_mut().expect("`PermissionRequest` is an array").push(json!({
        "matcher": "*",
        "hooks": [{
            "type": "http",
            "url": hook_url(),
            // Claude Code's default; the broker answers at 590s so it always replies first.
            "timeout": 600,
            "headers": { "Authorization": format!("Bearer {token}") }
        }]
    }));
    settings
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} isn't valid JSON ({e}); not touching it", path.display()))?;
            match value {
                Value::Object(_) => Ok(value),
                _ => Err(format!("{} isn't a JSON object; not touching it", path.display())),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    }
}

fn shape_ok(settings: &Value) -> Result<(), String> {
    let hooks = settings.get("hooks");
    if hooks.is_some_and(|h| !h.is_object()) {
        return Err("`hooks` in settings.json isn't an object; not touching it".into());
    }
    if hooks.and_then(|h| h.get("PermissionRequest")).is_some_and(|p| !p.is_array()) {
        return Err("`hooks.PermissionRequest` isn't an array; not touching it".into());
    }
    Ok(())
}

/// Keeps a copy of the previous file next to it, then replaces it in one rename.
fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    let err = |e: std::io::Error| format!("can't write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(err)?;
    }
    if path.exists() {
        fs::copy(path, path.with_extension("json.hoverpill-backup")).map_err(err)?;
    }
    let tmp = path.with_extension("json.hoverpill-tmp");
    let mut text = serde_json::to_string_pretty(settings).expect("serialising JSON can't fail");
    text.push('\n');
    fs::write(&tmp, text).map_err(err)?;
    fs::rename(&tmp, path).map_err(err)
}

pub fn install(path: &Path, token: &str) -> Result<(), String> {
    let settings = read_settings(path)?;
    shape_ok(&settings)?;
    write_settings(path, &with_hook(settings, token))
}

pub fn uninstall(path: &Path) -> Result<(), String> {
    let settings = read_settings(path)?;
    shape_ok(&settings)?;
    let updated = without_hook(settings.clone());
    if updated != settings {
        write_settings(path, &updated)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing() -> Value {
        json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "lint.sh" }] }],
                "PermissionRequest": [{ "matcher": "Edit", "hooks": [{ "type": "command", "command": "mine.sh" }] }]
            }
        })
    }

    #[test]
    fn install_keeps_other_settings_and_hooks() {
        let out = with_hook(existing(), "tok");
        assert_eq!(out["model"], "opus");
        assert_eq!(out["hooks"]["PreToolUse"], existing()["hooks"]["PreToolUse"]);
        let groups = out["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1]["hooks"][0]["headers"]["Authorization"], "Bearer tok");
    }

    #[test]
    fn reinstall_replaces_instead_of_duplicating() {
        let out = with_hook(with_hook(existing(), "old"), "new");
        let groups = out["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1]["hooks"][0]["headers"]["Authorization"], "Bearer new");
    }

    #[test]
    fn uninstall_restores_original() {
        assert_eq!(without_hook(with_hook(existing(), "tok")), existing());
        assert_eq!(without_hook(with_hook(json!({}), "tok")), json!({}));
    }

    #[test]
    fn install_and_uninstall_on_disk() {
        let dir = std::env::temp_dir().join(format!("hoverpill-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, serde_json::to_string(&existing()).unwrap()).unwrap();

        install(&path, "tok").unwrap();
        let installed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(installed["hooks"]["PermissionRequest"].as_array().unwrap().len(), 2);
        assert!(path.with_extension("json.hoverpill-backup").exists());

        uninstall(&path).unwrap();
        let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(restored, existing());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_invalid_json() {
        let dir = std::env::temp_dir().join(format!("hoverpill-bad-{}", std::process::id()));
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert!(install(&path, "tok").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::remove_dir_all(&dir).unwrap();
    }
}

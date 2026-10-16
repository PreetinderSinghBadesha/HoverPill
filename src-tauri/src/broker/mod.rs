//! Claude Code permission broker.
//!
//! Claude Code's `PermissionRequest` hook is configured as an `http` hook that POSTs to
//! `127.0.0.1:47823/claude/permission` (see `install.rs`). Each request becomes a card in the
//! pill and the HTTP response is held open until the user decides. If HoverPill isn't running,
//! or the user picks "Answer in terminal", Claude Code falls back to its normal terminal prompt.

pub mod install;
mod http;
mod summary;

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::events::{self, Decision, PermissionCancel, PermissionRequest};

pub const PORT: u16 = 47823;
pub const PATH: &str = "/claude/permission";

/// Below the hook's 600s timeout, so we reply (and clear the card) before Claude gives up.
const DECISION_TIMEOUT: Duration = Duration::from_secs(590);

const DENY_MESSAGE: &str =
    "The user denied this in HoverPill. Don't try to work around it; if you can't continue without it, stop and explain why.";

/// The parts of Claude Code's hook input we use.
#[derive(Deserialize)]
struct HookInput {
    tool_name: String,
    #[serde(default)]
    tool_input: Value,
    cwd: Option<String>,
    #[serde(default)]
    permission_suggestions: Vec<events::PermissionSuggestion>,
}

pub struct Broker {
    token: String,
    pending: Mutex<HashMap<String, Pending>>,
}

struct Pending {
    tx: oneshot::Sender<Decision>,
    req: PermissionRequest,
}

impl Broker {
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn Error>> {
        let dir = app.path().app_data_dir()?;
        Ok(Self { token: load_or_create_token(dir)?, pending: Mutex::default() })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns false if the request already went away (answered, timed out, or cancelled).
    pub fn resolve(&self, id: &str, decision: Decision) -> bool {
        match self.pending.lock().unwrap().remove(id) {
            Some(pending) => pending.tx.send(decision).is_ok(),
            None => false,
        }
    }

    fn get_request(&self, id: &str) -> Option<PermissionRequest> {
        self.pending.lock().unwrap().get(id).map(|p| p.req.clone())
    }

    fn register(&self, req: PermissionRequest) -> (String, oneshot::Receiver<Decision>) {
        let id = req.id.clone();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), Pending { tx, req });
        (id, rx)
    }

    fn forget(&self, id: &str) {
        self.pending.lock().unwrap().remove(id);
    }

    fn authorized(&self, header: Option<&str>) -> bool {
        let Some(given) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
            return false;
        };
        // Constant-time comparison.
        given.len() == self.token.len()
            && given.bytes().zip(self.token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

fn load_or_create_token(dir: PathBuf) -> Result<String, Box<dyn Error>> {
    let path = dir.join("claude-hook-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if existing.len() >= 32 {
            return Ok(existing.to_string());
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(token)
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("hoverpill: broker can't listen on {PORT}: {e}");
                events::notify(&app, "Claude broker is off", &format!("Port {PORT} is already in use."));
                return;
            }
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = handle(&app, stream).await {
                    eprintln!("hoverpill: broker connection error: {e}");
                }
            });
        }
    });
}

/// Browsers always send `Origin` on cross-site POSTs, and a DNS-rebinding page can't fake
/// `Host`, so together these keep web pages out even though we only listen on loopback.
fn from_browser_or_foreign_host(req: &http::Request) -> bool {
    let host_ok = matches!(req.header("host"), Some(h) if h == format!("127.0.0.1:{PORT}") || h == format!("localhost:{PORT}"));
    req.header("origin").is_some() || !host_ok
}

async fn handle(app: &AppHandle, mut stream: TcpStream) -> std::io::Result<()> {
    let req = match http::read_request(&mut stream).await {
        Ok(r) => r,
        Err(e) => return http::respond(&mut stream, 400, "text/plain", e.as_bytes()).await,
    };
    if from_browser_or_foreign_host(&req) {
        return http::respond(&mut stream, 403, "text/plain", b"forbidden").await;
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => return http::respond(&mut stream, 200, "text/plain", b"ok").await,
        ("POST", PATH) => {}
        _ => return http::respond(&mut stream, 404, "text/plain", b"not found").await,
    }

    let broker = app.state::<Broker>();
    if !broker.authorized(req.header("authorization")) {
        events::notify(app, "Claude hook rejected", "Token mismatch. Reinstall the hook from the HoverPill tray menu.");
        return http::respond(&mut stream, 401, "text/plain", b"bad token").await;
    }
    let input: HookInput = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(e) => return http::respond(&mut stream, 400, "text/plain", e.to_string().as_bytes()).await,
    };

    let summary = summary::summarize(&input.tool_name, &input.tool_input);
    let suggestions = if input.permission_suggestions.is_empty() { None } else { Some(input.permission_suggestions) };
    let req = PermissionRequest { id: events::next_id("claude"), tool: summary.tool, command: summary.text, cwd: input.cwd, permission_suggestions: suggestions };
    let req_for_send = req.clone();
    let (id, decision) = broker.register(req.clone());
    events::send(app, "pill:permission", req_for_send);
    let decision_result = tokio::select! {
        d = decision => d.unwrap_or(Decision::Ask),
        // The user interrupted Claude (or it timed out): nobody is listening for an answer.
        _ = http::closed(&mut stream) => {
            broker.forget(&id);
            events::send(app, "pill:permission-cancel", PermissionCancel { id: id.clone() });
            return Ok(());
        }
        _ = tokio::time::sleep(DECISION_TIMEOUT) => {
            broker.forget(&id);
            events::send(app, "pill:permission-cancel", PermissionCancel { id: id.clone() });
            Decision::Ask
        }
    };

    let pending_req = broker.get_request(&id);
    match hook_response(&decision_result, &pending_req.unwrap_or(req)) {
        Some(body) => http::respond(&mut stream, 200, "application/json", body.to_string().as_bytes()).await,
        // An empty 2xx is "no decision": Claude Code shows its usual terminal prompt.
        None => http::respond(&mut stream, 200, "text/plain", b"").await,
    }
}

fn hook_response(decision: &Decision, request: &PermissionRequest) -> Option<Value> {
    match decision {
        Decision::Allow => {
            Some(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } }))
        }
        Decision::Deny => {
            Some(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "deny", "message": DENY_MESSAGE } } }))
        }
        Decision::Ask => None,
        Decision::AlwaysAllow => {
            // Find the best suggestion - prefer addRules with allow behavior
            let mut updated_permissions = None;
            if let Some(ref suggestions) = request.permission_suggestions {
                for s in suggestions {
                    if s.behavior.as_deref() == Some("allow") && s.r#type.as_deref() == Some("addRules") {
                        updated_permissions = Some(json!([s]));
                        break;
                    }
                }
                if updated_permissions.is_none() && !suggestions.is_empty() {
                    // Just take the first suggestion if it's allow type
                    for s in suggestions {
                        if s.behavior.as_deref() == Some("allow") {
                            updated_permissions = Some(json!([s]));
                            break;
                        }
                    }
                }
            }
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {
                        "behavior": "allow",
                        "updatedPermissions": updated_permissions
                    }
                }
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn broker(token: &str) -> Broker {
        Broker { token: token.into(), pending: Mutex::default() }
    }

    #[test]
    fn responses_match_hook_schema() {
        let req = PermissionRequest { id: "t".into(), tool: "Bash".into(), command: "echo".into(), cwd: None, permission_suggestions: None };
        let allow = hook_response(&Decision::Allow, &req).unwrap();
        assert_eq!(allow["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
        assert_eq!(allow["hookSpecificOutput"]["decision"], json!({ "behavior": "allow" }));

        let deny = hook_response(&Decision::Deny, &req).unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(deny["hookSpecificOutput"]["decision"]["message"], DENY_MESSAGE);

        assert!(hook_response(&Decision::Ask, &req).is_none());
    }

    #[test]
    fn token_must_match_exactly() {
        let b = broker("secret-token");
        assert!(b.authorized(Some("Bearer secret-token")));
        assert!(!b.authorized(Some("Bearer secret-tokem")));
        assert!(!b.authorized(Some("Bearer secret")));
        assert!(!b.authorized(Some("secret-token")));
        assert!(!b.authorized(None));
    }

    #[test]
    fn resolve_delivers_once() {
        let b = broker("t");
        let dummy = PermissionRequest { id: "t".into(), tool: "Bash".into(), command: "echo".into(), cwd: None, permission_suggestions: None };
        let (id, mut rx) = b.register(dummy);
        assert!(b.resolve(&id, Decision::Deny));
        assert!(matches!(rx.try_recv(), Ok(Decision::Deny)));
        assert!(!b.resolve(&id, Decision::Allow), "a second answer must be ignored");
    }
}

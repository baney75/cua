//! Foreground delivery never takes over a BaneySpaces user's screen.
//!
//! BaneySpaces (a macOS workspace manager) can keep an agent workspace: one of its nine
//! workspaces where agents drive real apps while the person works on another. Its windows stay
//! parked out of view but usable, so the background routes (AX, pid-routed events) work on them.
//! `delivery_mode:"foreground"` would activate such a window and, with it, pull the person off
//! the workspace they're on, which is exactly what the agent workspace exists to prevent.
//!
//! So a foreground request for a window that BaneySpaces files under its agent workspace, while
//! the person is on another workspace, is refused before any event, naming the background route
//! instead. BaneySpaces publishes what this needs in
//! `~/Library/Application Support/BaneySpaces/state.json`: `current`, `agentWorkspace` and
//! `workspaces` ({"9": [{"id": 41612, ...}]}). Without that file, or without an agent workspace,
//! nothing changes.

use cua_driver_core::protocol::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

/// What the guard reads from BaneySpaces' state file.
#[derive(Debug, PartialEq)]
pub(crate) struct SpacesState {
    pub current: i64,
    pub agent: Option<i64>,
    pub home: HashMap<u32, i64>,
}

impl SpacesState {
    pub(crate) fn parse(json: &Value) -> Option<Self> {
        let current = json.get("current")?.as_i64()?;
        let agent = json
            .get("agentWorkspace")
            .and_then(Value::as_i64)
            .filter(|n| (1..=9).contains(n));
        let mut home = HashMap::new();
        if let Some(spaces) = json.get("workspaces").and_then(Value::as_object) {
            for (n, windows) in spaces {
                let Ok(n) = n.parse::<i64>() else { continue };
                for w in windows.as_array().into_iter().flatten() {
                    if let Some(id) = w.get("id").and_then(Value::as_u64) {
                        home.insert(id as u32, n);
                    }
                }
            }
        }
        Some(Self {
            current,
            agent,
            home,
        })
    }

    fn read() -> Option<Self> {
        let path = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)?
            .join("Library/Application Support/BaneySpaces/state.json");
        let data = std::fs::read(path).ok()?;
        Self::parse(&serde_json::from_slice(&data).ok()?)
    }
}

/// The refusal for `tool` delivering in the foreground to `window_id`, or `None`. Pure, for tests.
pub(crate) fn refusal_for(
    tool: &str,
    foreground: bool,
    window_id: Option<u32>,
    state: Option<&SpacesState>,
) -> Option<ToolResult> {
    if !foreground {
        return None;
    }
    let (window_id, state) = (window_id?, state?);
    let agent = state.agent?;
    if state.current == agent || state.home.get(&window_id) != Some(&agent) {
        return None;
    }
    Some(ToolResult::error(format!(
        "{tool}: window {window_id} is on the BaneySpaces agent workspace ({agent}) while the user is on workspace {}. \
         Foreground delivery would bring it over their screen. Use the background route instead: omit delivery_mode, \
         or target an element_token from get_window_state. Only the user moves to workspace {agent}.",
        state.current
    )))
}

/// Checks a tool call's arguments against BaneySpaces' current state.
pub(crate) fn check(tool: &str, args: &Value, foreground: bool) -> Option<ToolResult> {
    if !foreground {
        return None;
    }
    let window_id = args
        .get("window_id")
        .and_then(Value::as_u64)
        .map(|w| w as u32);
    refusal_for(tool, foreground, window_id, SpacesState::read().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(current: i64, agent: Option<i64>) -> SpacesState {
        let mut v = json!({"current": current, "workspaces": {"1": [{"app": "Ghostty", "id": 10}], "9": [{"app": "Helium", "id": 99}]}});
        if let Some(a) = agent {
            v["agentWorkspace"] = json!(a);
        }
        SpacesState::parse(&v).unwrap()
    }

    #[test]
    fn foreground_to_an_agent_window_from_another_workspace_is_refused() {
        let s = state(1, Some(9));
        let r = refusal_for("click", true, Some(99), Some(&s)).expect("refused");
        assert!(format!("{r:?}").contains("agent workspace (9)"));
    }

    #[test]
    fn background_routes_and_other_windows_pass() {
        let s = state(1, Some(9));
        assert!(
            refusal_for("click", false, Some(99), Some(&s)).is_none(),
            "background is what the agent workspace is for"
        );
        assert!(
            refusal_for("click", true, Some(10), Some(&s)).is_none(),
            "the user's own workspace"
        );
        assert!(refusal_for("click", true, None, Some(&s)).is_none());
    }

    #[test]
    fn on_the_agent_workspace_or_without_one_nothing_changes() {
        assert!(refusal_for("type_text", true, Some(99), Some(&state(9, Some(9)))).is_none());
        assert!(refusal_for("type_text", true, Some(99), Some(&state(1, None))).is_none());
        assert!(
            refusal_for("type_text", true, Some(99), None).is_none(),
            "no BaneySpaces"
        );
        assert_eq!(
            SpacesState::parse(&json!({"agentWorkspace": 9})),
            None,
            "a file without current is ignored"
        );
    }
}

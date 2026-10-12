//! Turns a hook's `tool_name` + `tool_input` into the two strings the permission card shows.

use serde_json::Value;

const MAX_TEXT: usize = 1500;

pub struct Summary {
    /// Shown in the header chip.
    pub tool: String,
    /// Shown in the monospace block.
    pub text: String,
}

fn str_field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

fn line_count(s: &str) -> usize {
    s.lines().count()
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_TEXT {
        s = s.chars().take(MAX_TEXT).collect();
        s.push('…');
    }
    s
}

pub fn summarize(tool: &str, input: &Value) -> Summary {
    let field = |k| str_field(input, k).unwrap_or("").to_string();
    let path = || field("file_path");
    let compact_json = || serde_json::to_string(input).unwrap_or_default();

    let text = match tool {
        "Bash" | "PowerShell" => field("command"),
        "Edit" => {
            let old = str_field(input, "old_string").map_or(0, line_count);
            let new = str_field(input, "new_string").map_or(0, line_count);
            let all = if input.get("replace_all").and_then(Value::as_bool) == Some(true) { ", all occurrences" } else { "" };
            format!("{}\n−{old} +{new} lines{all}", path())
        }
        "MultiEdit" => {
            let n = input.get("edits").and_then(Value::as_array).map_or(0, Vec::len);
            format!("{}\n{}", path(), plural(n, "edit"))
        }
        "Write" => {
            let n = str_field(input, "content").map_or(0, line_count);
            format!("{}\n{}", path(), plural(n, "line"))
        }
        "Read" => path(),
        "NotebookEdit" => field("notebook_path"),
        "Glob" | "Grep" => match str_field(input, "path") {
            Some(p) => format!("{}\nin {p}", field("pattern")),
            None => field("pattern"),
        },
        "WebFetch" => field("url"),
        "WebSearch" => field("query"),
        "Task" | "Agent" => field("description"),
        _ => compact_json(),
    };

    // MCP tools are named mcp__<server>__<tool>.
    let label = match tool.strip_prefix("mcp__").and_then(|rest| rest.split_once("__")) {
        Some((server, name)) => format!("{server} · {name}"),
        None => tool.to_string(),
    };

    let text = if text.trim().is_empty() { compact_json() } else { text };
    Summary { tool: label, text: truncate(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bash_shows_command() {
        let s = summarize("Bash", &json!({ "command": "rm -rf node_modules", "description": "clean" }));
        assert_eq!((s.tool.as_str(), s.text.as_str()), ("Bash", "rm -rf node_modules"));
    }

    #[test]
    fn edit_shows_path_and_diff_stat() {
        let s = summarize("Edit", &json!({ "file_path": "src/a.rs", "old_string": "a\nb", "new_string": "c" }));
        assert_eq!(s.text, "src/a.rs\n−2 +1 lines");
    }

    #[test]
    fn write_counts_lines() {
        let s = summarize("Write", &json!({ "file_path": "x.txt", "content": "one" }));
        assert_eq!(s.text, "x.txt\n1 line");
    }

    #[test]
    fn mcp_tool_label_and_json() {
        let s = summarize("mcp__github__create_issue", &json!({ "title": "Bug" }));
        assert_eq!(s.tool, "github · create_issue");
        assert_eq!(s.text, r#"{"title":"Bug"}"#);
    }

    #[test]
    fn missing_field_falls_back_to_json() {
        let s = summarize("Bash", &json!({ "cmd": "ls" }));
        assert_eq!(s.text, r#"{"cmd":"ls"}"#);
    }

    #[test]
    fn long_text_is_truncated() {
        let s = summarize("Bash", &json!({ "command": "x".repeat(5000) }));
        assert_eq!(s.text.chars().count(), MAX_TEXT + 1);
        assert!(s.text.ends_with('…'));
    }
}

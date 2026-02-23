use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use super::provider::{
    cutoff_from_days, is_recent, join_tool_uses_with_results, ExtractedCommand, SessionProvider,
    ToolSource, OUTPUT_PREVIEW_CHARS,
};

pub struct ClineProvider;

impl ClineProvider {
    fn task_dirs() -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return vec![];
        };

        let mut dirs = vec![home.join(".cline").join("tasks")];

        #[cfg(target_os = "macos")]
        {
            dirs.push(home.join(
                "Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/tasks",
            ));
        }
        #[cfg(target_os = "linux")]
        {
            dirs.push(home.join(".config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks"));
        }

        dirs
    }
}

impl SessionProvider for ClineProvider {
    fn tool_source(&self) -> ToolSource {
        ToolSource::Cline
    }

    fn discover_sessions(
        &self,
        _project_filter: Option<&str>,
        since_days: Option<u64>,
    ) -> Result<Vec<PathBuf>> {
        let cutoff = cutoff_from_days(since_days);
        let mut sessions = Vec::new();

        for task_dir in Self::task_dirs() {
            if !task_dir.exists() {
                continue;
            }

            let entries = match fs::read_dir(&task_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let history_file = path.join("api_conversation_history.json");
                if !history_file.exists() {
                    continue;
                }

                if !is_recent(&history_file, cutoff) {
                    continue;
                }

                sessions.push(history_file);
            }
        }

        Ok(sessions)
    }

    fn extract_commands(&self, path: &Path) -> Result<Vec<ExtractedCommand>> {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = BufReader::new(file);

        let messages: Vec<serde_json::Value> = serde_json::from_reader(reader)
            .with_context(|| format!("failed to parse {}", path.display()))?;

        let session_id = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut tool_uses: Vec<(String, String, usize)> = Vec::new();
        let mut tool_results: HashMap<String, (usize, String, bool)> = HashMap::new();
        let mut sequence_counter = 0;

        for msg in &messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let content = match msg.get("content").and_then(|c| c.as_array()) {
                Some(c) => c,
                None => continue,
            };

            match role {
                "assistant" => {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                            continue;
                        }
                        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name != "execute_command" {
                            continue;
                        }
                        let id = match block.get("id").and_then(|i| i.as_str()) {
                            Some(id) => id,
                            None => continue,
                        };
                        let command = block
                            .pointer("/input/command")
                            .and_then(|c| c.as_str())
                            .unwrap_or("");
                        if command.is_empty() {
                            continue;
                        }
                        tool_uses.push((id.to_string(), command.to_string(), sequence_counter));
                        sequence_counter += 1;
                    }
                }
                "user" => {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                            continue;
                        }
                        let id = match block.get("tool_use_id").and_then(|i| i.as_str()) {
                            Some(id) => id,
                            None => continue,
                        };
                        let output = block.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        let is_error = block
                            .get("is_error")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                        let preview: String = output.chars().take(OUTPUT_PREVIEW_CHARS).collect();
                        tool_results.insert(id.to_string(), (output.len(), preview, is_error));
                    }
                }
                _ => {}
            }
        }

        Ok(join_tool_uses_with_results(
            tool_uses,
            &tool_results,
            &session_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_json(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_extract_execute_command() {
        let json = make_json(
            r#"[
            {"role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"execute_command","input":{"command":"npm test"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"All tests passed","is_error":false}]}
        ]"#,
        );

        let provider = ClineProvider;
        let cmds = provider.extract_commands(json.path()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "npm test");
        assert!(!cmds[0].is_error);
        assert_eq!(cmds[0].output_len.unwrap(), "All tests passed".len());
    }

    #[test]
    fn test_non_execute_tools_ignored() {
        let json = make_json(
            r#"[
            {"role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"read_file","input":{"path":"/tmp/foo"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"file contents"}]}
        ]"#,
        );

        let provider = ClineProvider;
        let cmds = provider.extract_commands(json.path()).unwrap();
        assert_eq!(cmds.len(), 0);
    }

    #[test]
    fn test_error_command() {
        let json = make_json(
            r#"[
            {"role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"execute_command","input":{"command":"invalid_cmd"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"command not found","is_error":true}]}
        ]"#,
        );

        let provider = ClineProvider;
        let cmds = provider.extract_commands(json.path()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].is_error);
        assert!(cmds[0]
            .output_content
            .as_ref()
            .unwrap()
            .contains("command not found"));
    }

    #[test]
    fn test_multiple_commands() {
        let json = make_json(
            r#"[
            {"role":"assistant","content":[
                {"type":"tool_use","id":"tu_1","name":"execute_command","input":{"command":"git status"}},
                {"type":"tool_use","id":"tu_2","name":"execute_command","input":{"command":"git diff"}}
            ]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"tu_1","content":"clean"},
                {"type":"tool_result","tool_use_id":"tu_2","content":"no changes"}
            ]}
        ]"#,
        );

        let provider = ClineProvider;
        let cmds = provider.extract_commands(json.path()).unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].command, "git status");
        assert_eq!(cmds[1].command, "git diff");
        assert_eq!(cmds[0].sequence_index, 0);
        assert_eq!(cmds[1].sequence_index, 1);
    }

    #[test]
    fn test_empty_conversation() {
        let json = make_json("[]");

        let provider = ClineProvider;
        let cmds = provider.extract_commands(json.path()).unwrap();
        assert_eq!(cmds.len(), 0);
    }

    #[test]
    fn test_tool_source() {
        let provider = ClineProvider;
        assert_eq!(provider.tool_source(), ToolSource::Cline);
    }
}

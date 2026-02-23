use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const SECONDS_PER_DAY: u64 = 86_400;
/// Max chars to keep from tool output for error detection.
pub const OUTPUT_PREVIEW_CHARS: usize = 1000;

/// Which AI coding tool a session came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ToolSource {
    ClaudeCode,
    CodexCli,
    Cline,
    Cursor,
}

impl ToolSource {
    pub fn short_name(&self) -> &'static str {
        match self {
            ToolSource::ClaudeCode => "claude",
            ToolSource::CodexCli => "codex",
            ToolSource::Cline => "cline",
            ToolSource::Cursor => "cursor",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ToolSource::ClaudeCode => "Claude Code",
            ToolSource::CodexCli => "Codex CLI",
            ToolSource::Cline => "Cline",
            ToolSource::Cursor => "Cursor",
        }
    }
}

/// A command extracted from a session file.
#[derive(Debug)]
pub struct ExtractedCommand {
    pub command: String,
    pub output_len: Option<usize>,
    #[allow(dead_code)]
    pub session_id: String,
    /// Actual output content (first ~1000 chars for error detection)
    pub output_content: Option<String>,
    /// Whether the tool_result indicated an error
    pub is_error: bool,
    /// Chronological sequence index within the session
    pub sequence_index: usize,
}

/// Trait for session providers (Claude Code, Codex CLI, Cline, Cursor).
pub trait SessionProvider {
    fn tool_source(&self) -> ToolSource;
    fn discover_sessions(
        &self,
        project_filter: Option<&str>,
        since_days: Option<u64>,
    ) -> Result<Vec<PathBuf>>;
    fn extract_commands(&self, path: &Path) -> Result<Vec<ExtractedCommand>>;
}

/// Compute a mtime cutoff from a number of days ago.
pub fn cutoff_from_days(since_days: Option<u64>) -> Option<SystemTime> {
    since_days.map(|days| {
        SystemTime::now()
            .checked_sub(Duration::from_secs(days * SECONDS_PER_DAY))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    })
}

/// Check if a file's mtime is after the cutoff.
pub fn is_recent(path: &Path, cutoff: Option<SystemTime>) -> bool {
    let Some(cutoff_time) = cutoff else {
        return true;
    };
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(mtime) => mtime >= cutoff_time,
        Err(_) => false,
    }
}

/// Join collected tool_use entries with their tool_result responses by ID.
pub fn join_tool_uses_with_results(
    tool_uses: Vec<(String, String, usize)>,
    tool_results: &HashMap<String, (usize, String, bool)>,
    session_id: &str,
) -> Vec<ExtractedCommand> {
    tool_uses
        .into_iter()
        .map(|(tool_id, command, sequence_index)| {
            let (output_len, output_content, is_error) = tool_results
                .get(&tool_id)
                .map(|(len, content, err)| (Some(*len), Some(content.clone()), *err))
                .unwrap_or((None, None, false));

            ExtractedCommand {
                command,
                output_len,
                session_id: session_id.to_string(),
                output_content,
                is_error,
                sequence_index,
            }
        })
        .collect()
}

pub const VALID_TOOL_NAMES: &[&str] = &["claude", "codex", "cline", "cursor"];

/// Build the list of session providers, optionally filtered by tool short name.
pub fn build_providers(tool_filter: Option<&str>) -> Vec<Box<dyn SessionProvider>> {
    use super::cline_provider::ClineProvider;
    use super::codex_provider::CodexProvider;
    use super::cursor_provider::CursorProvider;

    let all: Vec<Box<dyn SessionProvider>> = vec![
        Box::new(ClaudeProvider),
        Box::new(CodexProvider),
        Box::new(ClineProvider),
        Box::new(CursorProvider),
    ];

    match tool_filter {
        Some(filter) => all
            .into_iter()
            .filter(|p| p.tool_source().short_name() == filter)
            .collect(),
        None => all,
    }
}

pub struct ClaudeProvider;

impl ClaudeProvider {
    /// Get the base directory for Claude Code projects.
    fn projects_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("could not determine home directory")?;
        let dir = home.join(".claude").join("projects");
        if !dir.exists() {
            anyhow::bail!(
                "Claude Code projects directory not found: {}\nMake sure Claude Code has been used at least once.",
                dir.display()
            );
        }
        Ok(dir)
    }

    /// Encode a filesystem path to Claude Code's directory name format.
    /// `/Users/foo/bar` → `-Users-foo-bar`
    pub fn encode_project_path(path: &str) -> String {
        path.replace('/', "-")
    }
}

impl SessionProvider for ClaudeProvider {
    fn tool_source(&self) -> ToolSource {
        ToolSource::ClaudeCode
    }

    fn discover_sessions(
        &self,
        project_filter: Option<&str>,
        since_days: Option<u64>,
    ) -> Result<Vec<PathBuf>> {
        let projects_dir = Self::projects_dir()?;
        let cutoff = cutoff_from_days(since_days);

        // For Claude, encode the project filter to match directory name format
        let encoded_filter = project_filter.map(|f| Self::encode_project_path(f));

        let mut sessions = Vec::new();

        let entries = fs::read_dir(&projects_dir)
            .with_context(|| format!("failed to read {}", projects_dir.display()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(ref filter) = encoded_filter {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !dir_name.contains(filter.as_str()) {
                    continue;
                }
            }

            // Walk the project directory recursively (catches subagents/)
            for walk_entry in WalkDir::new(&path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let file_path = walk_entry.path();
                if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }

                if !is_recent(file_path, cutoff) {
                    continue;
                }

                sessions.push(file_path.to_path_buf());
            }
        }

        Ok(sessions)
    }

    fn extract_commands(&self, path: &Path) -> Result<Vec<ExtractedCommand>> {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = BufReader::new(file);

        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut pending_tool_uses: Vec<(String, String, usize)> = Vec::new();
        let mut tool_results: HashMap<String, (usize, String, bool)> = HashMap::new();
        let mut sequence_counter = 0;

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };

            // Pre-filter: skip lines that can't contain Bash tool_use or tool_result
            if !line.contains("\"Bash\"") && !line.contains("\"tool_result\"") {
                continue;
            }

            let entry: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match entry_type {
                "assistant" => {
                    // Look for tool_use Bash blocks in message.content
                    if let Some(content) =
                        entry.pointer("/message/content").and_then(|c| c.as_array())
                    {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                                && block.get("name").and_then(|n| n.as_str()) == Some("Bash")
                            {
                                if let (Some(id), Some(cmd)) = (
                                    block.get("id").and_then(|i| i.as_str()),
                                    block.pointer("/input/command").and_then(|c| c.as_str()),
                                ) {
                                    pending_tool_uses.push((
                                        id.to_string(),
                                        cmd.to_string(),
                                        sequence_counter,
                                    ));
                                    sequence_counter += 1;
                                }
                            }
                        }
                    }
                }
                "user" => {
                    // Look for tool_result blocks
                    if let Some(content) =
                        entry.pointer("/message/content").and_then(|c| c.as_array())
                    {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str())
                                {
                                    let content =
                                        block.get("content").and_then(|c| c.as_str()).unwrap_or("");
                                    let output_len = content.len();
                                    let is_error = block
                                        .get("is_error")
                                        .and_then(|e| e.as_bool())
                                        .unwrap_or(false);
                                    let content_preview: String =
                                        content.chars().take(OUTPUT_PREVIEW_CHARS).collect();

                                    tool_results.insert(
                                        id.to_string(),
                                        (output_len, content_preview, is_error),
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(join_tool_uses_with_results(
            pending_tool_uses,
            &tool_results,
            &session_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_extract_assistant_bash() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_abc","name":"Bash","input":{"command":"git status"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_abc","content":"On branch master\nnothing to commit"}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "git status");
        assert!(cmds[0].output_len.is_some());
        assert_eq!(
            cmds[0].output_len.unwrap(),
            "On branch master\nnothing to commit".len()
        );
    }

    #[test]
    fn test_extract_non_bash_ignored() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_abc","name":"Read","input":{"file_path":"/tmp/foo"}}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 0);
    }

    #[test]
    fn test_extract_non_message_ignored() {
        let jsonl =
            make_jsonl(&[r#"{"type":"file-history-snapshot","messageId":"abc","snapshot":{}}"#]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 0);
    }

    #[test]
    fn test_extract_multiple_tools() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"git status"}},{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"git diff"}}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].command, "git status");
        assert_eq!(cmds[1].command, "git diff");
    }

    #[test]
    fn test_extract_malformed_line() {
        let jsonl = make_jsonl(&[
            "this is not json at all",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_ok","name":"Bash","input":{"command":"ls"}}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "ls");
    }

    #[test]
    fn test_encode_project_path() {
        assert_eq!(
            ClaudeProvider::encode_project_path("/Users/foo/bar"),
            "-Users-foo-bar"
        );
    }

    #[test]
    fn test_encode_project_path_trailing_slash() {
        assert_eq!(
            ClaudeProvider::encode_project_path("/Users/foo/bar/"),
            "-Users-foo-bar-"
        );
    }

    #[test]
    fn test_match_project_filter() {
        let encoded = ClaudeProvider::encode_project_path("/Users/foo/Sites/rtk");
        assert!(encoded.contains("rtk"));
        assert!(encoded.contains("Sites"));
    }

    #[test]
    fn test_extract_output_content() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_abc","name":"Bash","input":{"command":"git commit --ammend"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_abc","content":"error: unexpected argument '--ammend'","is_error":true}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "git commit --ammend");
        assert!(cmds[0].is_error);
        assert!(cmds[0].output_content.is_some());
        assert_eq!(
            cmds[0].output_content.as_ref().unwrap(),
            "error: unexpected argument '--ammend'"
        );
    }

    #[test]
    fn test_extract_is_error_flag() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}},{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"invalid_cmd"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"file1.txt","is_error":false},{"type":"tool_result","tool_use_id":"toolu_2","content":"command not found","is_error":true}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(!cmds[0].is_error);
        assert_eq!(cmds[1].is_error, true);
    }

    #[test]
    fn test_extract_sequence_ordering() {
        let jsonl = make_jsonl(&[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"first"}},{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"second"}},{"type":"tool_use","id":"toolu_3","name":"Bash","input":{"command":"third"}}]}}"#,
        ]);

        let provider = ClaudeProvider;
        let cmds = provider.extract_commands(jsonl.path()).unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].sequence_index, 0);
        assert_eq!(cmds[1].sequence_index, 1);
        assert_eq!(cmds[2].sequence_index, 2);
        assert_eq!(cmds[0].command, "first");
        assert_eq!(cmds[1].command, "second");
        assert_eq!(cmds[2].command, "third");
    }

    #[test]
    fn test_tool_source_short_names() {
        assert_eq!(ToolSource::ClaudeCode.short_name(), "claude");
        assert_eq!(ToolSource::CodexCli.short_name(), "codex");
        assert_eq!(ToolSource::Cline.short_name(), "cline");
        assert_eq!(ToolSource::Cursor.short_name(), "cursor");
    }

    #[test]
    fn test_tool_source_display_names() {
        assert_eq!(ToolSource::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(ToolSource::CodexCli.display_name(), "Codex CLI");
        assert_eq!(ToolSource::Cline.display_name(), "Cline");
        assert_eq!(ToolSource::Cursor.display_name(), "Cursor");
    }

    #[test]
    fn test_build_providers_all() {
        let providers = build_providers(None);
        assert_eq!(providers.len(), 4);
    }

    #[test]
    fn test_build_providers_filtered() {
        let providers = build_providers(Some("claude"));
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].tool_source(), ToolSource::ClaudeCode);
    }

    #[test]
    fn test_build_providers_unknown_filter() {
        let providers = build_providers(Some("unknown"));
        assert_eq!(providers.len(), 0);
    }

    #[test]
    fn test_claude_provider_tool_source() {
        let provider = ClaudeProvider;
        assert_eq!(provider.tool_source(), ToolSource::ClaudeCode);
    }
}

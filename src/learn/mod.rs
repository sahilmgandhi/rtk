pub mod detector;
pub mod report;

use crate::discover::provider::{build_providers, VALID_TOOL_NAMES};
use anyhow::Result;
use detector::{deduplicate_corrections, find_corrections, CommandExecution};
use report::{format_console_report, write_rules_file};

pub fn run(
    project: Option<String>,
    all: bool,
    since: u64,
    format: String,
    write_rules: bool,
    min_confidence: f64,
    min_occurrences: usize,
    tool: Option<String>,
) -> Result<()> {
    if let Some(ref t) = tool {
        if !VALID_TOOL_NAMES.contains(&t.as_str()) {
            anyhow::bail!(
                "Unknown tool '{}'. Valid options: {}",
                t,
                VALID_TOOL_NAMES.join(", ")
            );
        }
    }

    let providers = build_providers(tool.as_deref());

    let project_filter = if all {
        None
    } else if let Some(p) = project {
        Some(p)
    } else {
        let cwd = std::env::current_dir()?;
        Some(cwd.to_string_lossy().to_string())
    };

    let mut total_sessions: usize = 0;
    let mut all_commands: Vec<CommandExecution> = Vec::new();

    for provider in &providers {
        let sessions = match provider.discover_sessions(project_filter.as_deref(), Some(since)) {
            Ok(s) => s,
            Err(_) => continue,
        };

        total_sessions += sessions.len();

        for session_path in &sessions {
            let extracted = match provider.extract_commands(session_path) {
                Ok(cmds) => cmds,
                Err(_) => continue,
            };

            for ext_cmd in extracted {
                if let Some(output) = ext_cmd.output_content {
                    all_commands.push(CommandExecution {
                        command: ext_cmd.command,
                        is_error: ext_cmd.is_error,
                        output,
                    });
                }
            }
        }
    }

    if total_sessions == 0 {
        println!("No AI coding sessions found in the last {} days.", since);
        return Ok(());
    }

    // Sort by sequence index to maintain chronological order
    // (already sorted by extraction order within each session)

    // Find corrections
    let corrections = find_corrections(&all_commands);

    if corrections.is_empty() {
        println!(
            "No CLI corrections detected in {} sessions.",
            total_sessions
        );
        return Ok(());
    }

    // Filter by confidence
    let filtered: Vec<_> = corrections
        .into_iter()
        .filter(|c| c.confidence >= min_confidence)
        .collect();

    // Deduplicate
    let mut rules = deduplicate_corrections(filtered.clone());

    // Filter by occurrences
    rules.retain(|r| r.occurrences >= min_occurrences);

    // Output
    match format.as_str() {
        "json" => {
            // JSON output
            let json = serde_json::json!({
                "sessions_scanned": total_sessions,
                "total_corrections": filtered.len(),
                "rules": rules.iter().map(|r| serde_json::json!({
                    "wrong": r.wrong_pattern,
                    "right": r.right_pattern,
                    "error_type": r.error_type.as_str(),
                    "occurrences": r.occurrences,
                    "base_command": r.base_command,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        _ => {
            // Text output
            let report = format_console_report(&rules, filtered.len(), total_sessions, since);
            print!("{}", report);

            if write_rules && !rules.is_empty() {
                let rules_path = ".claude/rules/cli-corrections.md";
                write_rules_file(&rules, rules_path)?;
                println!("\nWritten to: {}", rules_path);
            }
        }
    }

    Ok(())
}

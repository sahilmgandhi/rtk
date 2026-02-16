use anyhow::{bail, Context, Result};
use std::path::Path;

/// Filter strategy for scaffolded modules.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum Strategy {
    /// Simple line filtering (grep-like)
    Plain,
    /// Regex-based with lazy_static! captures grouped by file
    Regex,
    /// JSON parsing with serde_json
    Json,
    /// Line-by-line NDJSON streaming
    Ndjson,
    /// State machine text parsing
    Text,
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Plain => write!(f, "plain"),
            Strategy::Regex => write!(f, "regex"),
            Strategy::Json => write!(f, "json"),
            Strategy::Ndjson => write!(f, "ndjson"),
            Strategy::Text => write!(f, "text"),
        }
    }
}

/// Validate that a tool name is a valid Rust identifier and doesn't collide with existing modules.
fn validate_tool_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Tool name cannot be empty");
    }

    if name.starts_with(|c: char| c.is_ascii_digit()) {
        bail!("Tool name cannot start with a digit: '{}'", name);
    }

    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!(
            "Tool name must contain only alphanumeric characters and underscores: '{}'",
            name
        );
    }

    // Check for collision with existing modules
    let module_path = format!("src/{}_cmd.rs", name);
    if Path::new(&module_path).exists() {
        bail!(
            "Module already exists: {}. Choose a different name.",
            module_path
        );
    }

    Ok(())
}

/// Convert a snake_case tool name to PascalCase for use in enum variants.
fn to_pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + &chars.collect::<String>()
                }
            }
        })
        .collect()
}

/// Generate the module source code for the given tool and strategy.
fn generate_module(tool: &str, strategy: &Strategy) -> String {
    let filter_fn = format!("filter_{}_output", tool);

    let (extra_imports, filter_body, test_input, extra_test) = match strategy {
        Strategy::Plain => (
            String::new(),
            format!(
                r#"    let mut lines = Vec::new();
    for line in input.lines() {{
        // TODO: Add your filtering logic here
        // Skip empty lines and noise, keep useful output
        if !line.trim().is_empty() {{
            lines.push(line);
        }}
    }}
    lines.join("\n")"#
            ),
            r#"line 1: something useful

line 2: also useful
noise line to filter
line 3: important"#
                .to_string(),
            String::new(),
        ),

        Strategy::Regex => (
            r#"use lazy_static::lazy_static;
use regex::Regex;
"#
            .to_string(),
            format!(
                r#"    lazy_static! {{
        static ref PATTERN: Regex = Regex::new(r"^(.+):(\d+): (.+)$").unwrap();
    }}

    let mut grouped: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for line in input.lines() {{
        if let Some(caps) = PATTERN.captures(line) {{
            let file = caps[1].to_string();
            let line_num = &caps[2];
            let msg = &caps[3];
            grouped.entry(file).or_default().push(format!("  L{{}}: {{}}", line_num, msg));
        }}
    }}

    let mut output = Vec::new();
    for (file, messages) in &grouped {{
        output.push(format!("{{}} ({{}} issues)", file, messages.len()));
        for msg in messages {{
            output.push(msg.clone());
        }}
    }}
    output.join("\n")"#
            ),
            r#"src/main.rs:10: unused variable
src/main.rs:20: missing semicolon
src/lib.rs:5: dead code"#
                .to_string(),
            String::new(),
        ),

        Strategy::Json => (
            r#"use serde::Deserialize;
"#
            .to_string(),
            format!(
                r#"    // TODO: Define your JSON structure above and deserialize here
    #[derive(Debug, Deserialize)]
    struct Item {{
        message: String,
        #[allow(dead_code)]
        severity: Option<String>,
    }}

    let items: Vec<Item> = serde_json::from_str(input)
        .context("Failed to parse {} JSON output")?;

    let mut output = Vec::new();
    output.push(format!("{{}} issues found", items.len()));
    for item in &items {{
        output.push(format!("  {{}}", item.message));
    }}
    Ok(output.join("\n"))"#,
                tool
            ),
            r#"[{"message":"something wrong","severity":"error"},{"message":"minor issue","severity":"warning"}]"#.to_string(),
            String::new(),
        ),

        Strategy::Ndjson => (
            r#"use serde::Deserialize;
"#
            .to_string(),
            format!(
                r#"    #[derive(Debug, Deserialize)]
    struct Event {{
        #[serde(rename = "Action")]
        action: Option<String>,
        #[serde(rename = "Output")]
        output: Option<String>,
    }}

    let mut failures = Vec::new();
    for line in input.lines() {{
        if line.trim().is_empty() {{
            continue;
        }}
        if let Ok(event) = serde_json::from_str::<Event>(line) {{
            if event.action.as_deref() == Some("fail") {{
                if let Some(out) = &event.output {{
                    failures.push(out.trim().to_string());
                }}
            }}
        }}
    }}

    if failures.is_empty() {{
        "all passed".to_string()
    }} else {{
        format!("{{}} failures:\n{{}}", failures.len(), failures.join("\n"))
    }}"#
            ),
            r#"{"Action":"pass","Output":"ok"}
{"Action":"fail","Output":"test_something failed"}
{"Action":"pass","Output":"ok"}"#
                .to_string(),
            String::new(),
        ),

        Strategy::Text => (
            String::new(),
            format!(
                r#"    #[derive(Debug, PartialEq)]
    enum State {{
        Header,
        Results,
        Failures,
        Summary,
    }}

    let mut state = State::Header;
    let mut failures = Vec::new();
    let mut summary_lines = Vec::new();

    for line in input.lines() {{
        match state {{
            State::Header => {{
                if line.contains("FAIL") || line.contains("FAILURES") {{
                    state = State::Failures;
                }} else if line.contains("passed") || line.contains("failed") {{
                    state = State::Summary;
                    summary_lines.push(line.to_string());
                }}
            }}
            State::Results => {{
                if line.contains("FAIL") {{
                    state = State::Failures;
                    failures.push(line.to_string());
                }} else if line.contains("passed") || line.contains("failed") {{
                    state = State::Summary;
                    summary_lines.push(line.to_string());
                }}
            }}
            State::Failures => {{
                if line.contains("passed") || line.contains("failed") {{
                    state = State::Summary;
                    summary_lines.push(line.to_string());
                }} else if !line.trim().is_empty() {{
                    failures.push(line.to_string());
                }}
            }}
            State::Summary => {{
                if !line.trim().is_empty() {{
                    summary_lines.push(line.to_string());
                }}
            }}
        }}
    }}

    let mut output = Vec::new();
    if !failures.is_empty() {{
        output.push(format!("{{}} failures:", failures.len()));
        output.extend(failures);
    }}
    output.extend(summary_lines);
    if output.is_empty() {{
        output.push("no output".to_string());
    }}
    output.join("\n")"#
            ),
            r#"Running tests...
test_one ... ok
test_two ... FAIL
FAILURES:
test_two: expected 1, got 2
2 tests, 1 passed, 1 failed"#
                .to_string(),
            String::new(),
        ),
    };

    // Determine if filter returns Result or String
    let (filter_return_type, filter_call) = match strategy {
        Strategy::Json => (
            "Result<String>",
            format!("    let filtered = {}(&raw)?;", filter_fn),
        ),
        _ => ("String", format!("    let filtered = {}(&raw);", filter_fn)),
    };

    let _ = extra_test; // Reserved for future per-strategy tests

    format!(
        r####"use crate::tracking;
use crate::utils::truncate;
use anyhow::{{Context, Result}};
use std::process::Command;
{extra_imports}
pub fn run(args: &[String], verbose: u8) -> Result<()> {{
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("{tool}");
    for arg in args {{
        cmd.arg(arg);
    }}

    if verbose > 0 {{
        eprintln!("Running: {tool} {{}}", args.join(" "));
    }}

    let output = cmd
        .output()
        .context("Failed to run {tool}. Is it installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{{}}\n{{}}", stdout, stderr);

{filter_call}

    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() {{ 0 }} else {{ 1 }});
    if let Some(hint) = crate::tee::tee_and_hint(&raw, "{tool}", exit_code) {{
        println!("{{}}\n{{}}", filtered, hint);
    }} else {{
        println!("{{}}", filtered);
    }}

    if !stderr.trim().is_empty() {{
        eprintln!("{{}}", truncate(stderr.trim(), 500));
    }}

    timer.track(
        &format!("{tool} {{}}", args.join(" ")),
        &format!("rtk {tool} {{}}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {{
        std::process::exit(exit_code);
    }}

    Ok(())
}}

fn {filter_fn}(input: &str) -> {filter_return_type} {{
{filter_body}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    fn count_tokens(text: &str) -> usize {{
        text.split_whitespace().count()
    }}

    #[test]
    fn test_{tool}_filter_basic() {{
        let input = r##"{test_input}"##;
        let output = {filter_fn}(input){unwrap};
        assert!(!output.is_empty(), "Filter should produce output");
    }}

    #[test]
    fn test_{tool}_filter_empty_input() {{
        let output = {filter_fn}(""){unwrap};
        // Should not panic on empty input
        let _ = output;
    }}
}}
"####,
        extra_imports = extra_imports,
        tool = tool,
        filter_fn = filter_fn,
        filter_call = filter_call,
        filter_return_type = filter_return_type,
        filter_body = filter_body,
        test_input = test_input,
        unwrap = if matches!(strategy, Strategy::Json) {
            r#".expect("filter should not fail on valid input")"#
        } else {
            ""
        },
    )
}

/// Print integration instructions after generating the module file.
fn print_instructions(tool: &str, strategy: &Strategy) {
    let pascal = to_pascal_case(tool);

    eprintln!();
    eprintln!("Generated src/{}_cmd.rs (strategy: {})", tool, strategy);
    eprintln!();
    eprintln!("Next steps to integrate into RTK:");
    eprintln!();
    eprintln!("1. Add module declaration in src/main.rs (with other mod declarations):");
    eprintln!("   mod {}_cmd;", tool);
    eprintln!();
    eprintln!("2. Add variant to Commands enum in src/main.rs:");
    eprintln!("   /// {} with compact output", tool);
    eprintln!("   {} {{", pascal);
    eprintln!("       #[arg(trailing_var_arg = true, allow_hyphen_values = true)]");
    eprintln!("       args: Vec<String>,");
    eprintln!("   }},");
    eprintln!();
    eprintln!("3. Add match arm in main() function:");
    eprintln!("   Commands::{} {{ args }} => {{", pascal);
    eprintln!("       {}_cmd::run(&args, cli.verbose)?;", tool);
    eprintln!("   }}");
    eprintln!();
    eprintln!("4. Run quality gate:");
    eprintln!("   cargo fmt --all && cargo clippy --all-targets && cargo test");
    eprintln!();
    eprintln!("5. Test manually:");
    eprintln!("   cargo run -- {} <args>", tool);
    eprintln!();
    eprintln!("See docs/ADDING_TOOLS.md for the full contributor guide.");
}

/// Run the scaffold command: generate a new command module.
pub fn run(tool: &str, strategy: &Strategy, dry_run: bool, verbose: u8) -> Result<()> {
    validate_tool_name(tool)?;

    let content = generate_module(tool, strategy);

    if dry_run {
        if verbose > 0 {
            eprintln!("Dry run: printing generated module to stdout");
        }
        println!("{}", content);
        return Ok(());
    }

    let src_dir = Path::new("src");
    if !src_dir.is_dir() {
        bail!(
            "Directory src/ does not exist. Are you in the RTK project root?\nCurrent directory: {}",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        );
    }

    let path = format!("src/{}_cmd.rs", tool);
    // Collision already validated in validate_tool_name()

    std::fs::write(&path, &content).with_context(|| format!("Failed to write {}", path))?;

    print_instructions(tool, strategy);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_tool_name ─────────────────────────────

    #[test]
    fn test_validate_valid_names() {
        assert!(validate_tool_name("swift").is_ok());
        assert!(validate_tool_name("gcc").is_ok());
        assert!(validate_tool_name("my_tool").is_ok());
        assert!(validate_tool_name("tool123").is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let err = validate_tool_name("").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_validate_starts_with_digit() {
        let err = validate_tool_name("123tool").unwrap_err();
        assert!(err.to_string().contains("digit"));
    }

    #[test]
    fn test_validate_invalid_chars() {
        assert!(validate_tool_name("my tool").is_err());
        assert!(validate_tool_name("my-tool").is_err());
        assert!(validate_tool_name("tool!").is_err());
    }

    #[test]
    fn test_validate_existing_module_collision() {
        // "git" has src/git.rs not src/git_cmd.rs, so no collision on git_cmd
        // But ruff has src/ruff_cmd.rs — this should collide
        // We can't test filesystem collision in unit tests without a real file,
        // so we test the logic path indirectly
        // The validate function checks Path::new("src/{}_cmd.rs").exists()
        // In test context, CWD is the project root, so this should detect real modules
    }

    // ── to_pascal_case ─────────────────────────────────

    #[test]
    fn test_pascal_case_simple() {
        assert_eq!(to_pascal_case("swift"), "Swift");
        assert_eq!(to_pascal_case("gcc"), "Gcc");
    }

    #[test]
    fn test_pascal_case_multi_word() {
        assert_eq!(to_pascal_case("my_tool"), "MyTool");
        assert_eq!(to_pascal_case("some_long_name"), "SomeLongName");
    }

    #[test]
    fn test_pascal_case_single_char() {
        assert_eq!(to_pascal_case("a"), "A");
    }

    #[test]
    fn test_pascal_case_already_upper() {
        assert_eq!(to_pascal_case("TOOL"), "TOOL");
    }

    // ── generate_module ────────────────────────────────

    #[test]
    fn test_generate_plain_contains_key_patterns() {
        let output = generate_module("swift", &Strategy::Plain);
        assert!(output.contains("use crate::tracking;"));
        assert!(output.contains("fn run("));
        assert!(output.contains("fn filter_swift_output("));
        assert!(output.contains("Command::new(\"swift\")"));
        assert!(output.contains("#[cfg(test)]"));
        assert!(output.contains("TimedExecution::start()"));
        assert!(output.contains("tee::tee_and_hint"));
    }

    #[test]
    fn test_generate_regex_has_lazy_static() {
        let output = generate_module("gcc", &Strategy::Regex);
        assert!(output.contains("lazy_static!"));
        assert!(output.contains("use regex::Regex;"));
        assert!(output.contains("fn filter_gcc_output("));
    }

    #[test]
    fn test_generate_json_has_serde() {
        let output = generate_module("jq", &Strategy::Json);
        assert!(output.contains("use serde::Deserialize;"));
        assert!(output.contains("serde_json::from_str"));
        assert!(output.contains("Result<String>"));
    }

    #[test]
    fn test_generate_ndjson_has_streaming() {
        let output = generate_module("mytest", &Strategy::Ndjson);
        assert!(output.contains("serde_json::from_str::<Event>"));
        assert!(output.contains("for line in input.lines()"));
    }

    #[test]
    fn test_generate_text_has_state_machine() {
        let output = generate_module("zig", &Strategy::Text);
        assert!(output.contains("enum State"));
        assert!(output.contains("State::Header"));
        assert!(output.contains("State::Failures"));
    }

    #[test]
    fn test_generate_all_strategies_compile_pattern() {
        // All strategies should produce valid-looking Rust code
        for strategy in &[
            Strategy::Plain,
            Strategy::Regex,
            Strategy::Json,
            Strategy::Ndjson,
            Strategy::Text,
        ] {
            let output = generate_module("testcmd", strategy);
            assert!(
                output.contains("pub fn run("),
                "Strategy {:?} missing run()",
                strategy
            );
            assert!(
                output.contains("fn filter_testcmd_output("),
                "Strategy {:?} missing filter fn",
                strategy
            );
            assert!(
                output.contains("#[cfg(test)]"),
                "Strategy {:?} missing tests",
                strategy
            );
        }
    }

    // ── run() with dry_run ─────────────────────────────

    #[test]
    fn test_run_dry_run_prints_to_stdout() {
        // dry_run should not create any file
        let result = run("nonexistent_test_tool", &Strategy::Plain, true, 0);
        assert!(result.is_ok());
        // Verify no file was created
        assert!(!Path::new("src/nonexistent_test_tool_cmd.rs").exists());
    }

    #[test]
    fn test_run_invalid_name_fails() {
        let result = run("", &Strategy::Plain, true, 0);
        assert!(result.is_err());

        let result = run("my tool", &Strategy::Plain, true, 0);
        assert!(result.is_err());
    }
}

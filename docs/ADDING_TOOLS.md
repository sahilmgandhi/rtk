# Adding a New Tool to RTK

This guide walks you through adding a new command filter to RTK. The `rtk scaffold` command generates 60% of the boilerplate — this guide covers the remaining integration and testing.

## Quick Start

```bash
# 1. Generate the module
rtk scaffold swift --strategy text --dry-run  # preview first
rtk scaffold swift --strategy text             # generate src/swift_cmd.rs

# 2. Follow the printed instructions to wire it into main.rs
# 3. Run quality gate
cargo fmt --all && cargo clippy --all-targets && cargo test
```

## Strategy Decision Tree

Pick a strategy based on the tool's output format:

```
Does the tool support JSON output?
├── YES: Does it output a single JSON array/object?
│   ├── YES → json (e.g., ruff check --output-format=json)
│   └── NO: Line-by-line JSON (NDJSON)?
│       └── YES → ndjson (e.g., go test -json)
└── NO: Is the output structured with repeating patterns?
    ├── YES: Can you capture with regex (file:line: message)?
    │   ├── YES → regex (e.g., gcc, eslint default)
    │   └── NO: Does it have distinct phases (header/results/summary)?
    │       └── YES → text (e.g., pytest, swift test)
    └── NO → plain (simple line filtering)
```

### Strategy Reference

| Strategy | When to use | Example modules | Typical savings |
|----------|-------------|-----------------|-----------------|
| `plain` | Simple line filtering, grep-like | `grep_cmd.rs` | 60-70% |
| `regex` | Repeating `file:line: msg` patterns | `lint_cmd.rs`, `tsc_cmd.rs` | 80-85% |
| `json` | Tool supports `--output-format=json` | `ruff_cmd.rs`, `golangci_cmd.rs` | 80-90% |
| `ndjson` | Line-by-line JSON streaming | `go_cmd.rs` (test) | 85-90% |
| `text` | State machine for phased output | `pytest_cmd.rs`, `vitest_cmd.rs` | 90%+ |

## Step-by-Step Integration (9 Steps)

### Step 1: Scaffold the Module

```bash
rtk scaffold <tool> --strategy <strategy>
# Creates src/<tool>_cmd.rs with:
# - run() function with tracking + tee
# - filter function skeleton
# - Basic test scaffolding
```

### Step 2: Add Module Declaration

In `src/main.rs`, add with the other `mod` declarations (alphabetical order):

```rust
mod <tool>_cmd;
```

### Step 3: Add Command Variant

In the `Commands` enum in `src/main.rs`:

```rust
/// <Tool> with compact output
<PascalCase> {
    /// <Tool> arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
},
```

### Step 4: Add Match Arm

In the `main()` match statement:

```rust
Commands::<PascalCase> { args } => {
    <tool>_cmd::run(&args, cli.verbose)?;
}
```

### Step 5: Implement the Filter

Edit `src/<tool>_cmd.rs` and replace the TODO sections in the filter function with your actual filtering logic. Key principles:

- **Show failures, hide successes** (for test runners)
- **Group by file** (for linters/compilers)
- **Strip noise** (ASCII art, progress bars, blank lines)
- **Preserve exit codes** (critical for CI/CD)

### Step 6: Create Test Fixtures

Capture real output from the tool:

```bash
<tool> <typical-args> > tests/fixtures/<tool>_raw.txt 2>&1
```

Update tests to use the fixture:

```rust
#[test]
fn test_<tool>_filter_real_output() {
    let input = include_str!("../tests/fixtures/<tool>_raw.txt");
    let output = filter_<tool>_output(input);
    assert!(!output.is_empty());
}
```

### Step 7: Verify Token Savings

Add a token savings test (target: 60%+):

```rust
fn count_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

#[test]
fn test_<tool>_token_savings() {
    let input = include_str!("../tests/fixtures/<tool>_raw.txt");
    let output = filter_<tool>_output(input);
    let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
    assert!(savings >= 60.0, "Expected >=60% savings, got {:.1}%", savings);
}
```

### Step 8: Add Smoke Test

In `scripts/test-all.sh`, add a section:

```bash
if command -v <tool> &>/dev/null; then
    assert_help    "rtk <tool>"    rtk <tool> --help
    assert_ok      "rtk <tool>"    rtk <tool> <safe-args>
else
    skip "<tool> not installed"
fi
```

### Step 9: Run Quality Gate

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test
```

Then test manually:

```bash
cargo run -- <tool> <args>
# Verify output is condensed and readable
```

## Walkthrough: Adding `swift test`

Here's a complete example of adding Swift test support:

```bash
# 1. Scaffold
rtk scaffold swift --strategy text

# 2. Edit src/swift_cmd.rs
#    - Replace Command::new("swift") args to pass ["test"] by default
#    - Implement filter: keep only failures + summary line
#    - State machine: Header → TestResults → Failures → Summary

# 3. Wire in main.rs
#    mod swift_cmd;
#    Commands enum: Swift { args: Vec<String> }
#    Match arm: Commands::Swift { args } => swift_cmd::run(&args, cli.verbose)?;

# 4. Create fixture
swift test 2>&1 > tests/fixtures/swift_test_raw.txt

# 5. Add tests with fixture + token savings assertion

# 6. Quality gate
cargo fmt --all && cargo clippy --all-targets && cargo test

# 7. Manual test
cargo run -- swift test
```

## Common Patterns

### Forcing Tool Output Format

Many tools support JSON output. Force it for reliable parsing:

```rust
// ruff check: force JSON
cmd.arg("check").arg("--output-format=json");

// go test: force JSON
cmd.arg("test").arg("-json");

// golangci-lint: force JSON
cmd.arg("run").arg("--out-format=json");
```

### Graceful Fallback

If your filter fails, fall back to raw output:

```rust
let filtered = match filter_output(&raw) {
    Ok(f) => f,
    Err(e) => {
        if verbose > 0 {
            eprintln!("Filter failed: {}, showing raw output", e);
        }
        raw.to_string()
    }
};
```

### Tee for Large Outputs

For commands that produce large output, the tee system saves raw output to disk on failure so LLMs can re-read without re-running:

```rust
let exit_code = output.status.code().unwrap_or(1);
if let Some(hint) = crate::tee::tee_and_hint(&raw, "<tool>", exit_code) {
    println!("{}\n{}", filtered, hint);
}
```

This is already included in scaffolded modules.

## Checklist

- [ ] `rtk scaffold <tool> --strategy <s>` run
- [ ] Filter logic implemented (not just TODO)
- [ ] Module registered in `main.rs` (mod + enum + match)
- [ ] Test fixture from real command output
- [ ] Token savings test (>=60%)
- [ ] Smoke test in `scripts/test-all.sh`
- [ ] `cargo fmt --all && cargo clippy --all-targets && cargo test` passes
- [ ] Manual test: `cargo run -- <tool> <args>` output is correct
- [ ] README.md updated with new command

#!/bin/bash
# Auto-format Rust code before commits
# Hook: PreToolUse for git commit

echo "🦀 Running Rust pre-commit checks..."

# Format code
cargo fmt --all

# Check for compilation errors only (warnings allowed)
if cargo clippy --all-targets 2>&1 | grep -q "error:"; then
    echo "❌ Clippy found errors. Fix them before committing."
    exit 1
fi

# Validate documentation consistency (version, module count, commands)
if [ -f "scripts/validate-docs.sh" ]; then
    if ! bash scripts/validate-docs.sh; then
        echo "❌ Documentation validation failed. Fix before committing."
        exit 1
    fi
fi

echo "✅ Pre-commit checks passed (warnings allowed)"

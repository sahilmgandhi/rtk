#!/bin/bash
# Realistic human typing simulator for asciinema demo

# ANSI colors
RESET="\033[0m"
BOLD="\033[1m"
GREEN="\033[32m"
BLUE="\033[34m"
GRAY="\033[90m"

# Typing simulation with human-like timing
# Based on research: avg 100-130ms/char, 6% error rate, variable speed
human_type() {
    local text="$1"
    local base_delay=${2:-0.08}  # Base delay between chars (80ms)
    local i=0
    local len=${#text}
    local error_chance=6  # 6% error rate

    while [ $i -lt $len ]; do
        char="${text:$i:1}"

        # Random error injection (~6% chance)
        if [ $((RANDOM % 100)) -lt $error_chance ] && [ "$char" != " " ]; then
            # Type wrong char
            wrong_chars="qwertyuiopasdfghjklzxcvbnm"
            wrong_char="${wrong_chars:$((RANDOM % 26)):1}"
            printf "%s" "$wrong_char"

            # Variable pause before noticing error (100-400ms)
            sleep $(echo "scale=3; (100 + $RANDOM % 300) / 1000" | bc)

            # Backspace
            printf "\b \b"
            sleep 0.05
        fi

        # Print correct character
        printf "%s" "$char"

        # Variable delay based on character type (human rhythm)
        # Spaces and punctuation: longer pause (thinking)
        # Common bigrams: faster (muscle memory)
        case "$char" in
            " ")
                # Word boundary: 80-200ms
                sleep $(echo "scale=3; (80 + $RANDOM % 120) / 1000" | bc)
                ;;
            "." | "," | "-" | "/")
                # Punctuation: 100-250ms
                sleep $(echo "scale=3; (100 + $RANDOM % 150) / 1000" | bc)
                ;;
            *)
                # Normal char: 50-150ms with variation
                # Faster for common letters, slower for rare
                case "$char" in
                    [etaoins])
                        sleep $(echo "scale=3; (40 + $RANDOM % 60) / 1000" | bc)
                        ;;
                    [rhldcu])
                        sleep $(echo "scale=3; (50 + $RANDOM % 70) / 1000" | bc)
                        ;;
                    *)
                        sleep $(echo "scale=3; (60 + $RANDOM % 90) / 1000" | bc)
                        ;;
                esac
                ;;
        esac

        i=$((i + 1))
    done
}

# Type command with prompt
type_cmd() {
    printf "${GREEN}\$${RESET} "
    human_type "$1"
    echo ""
    # Pause before execution (like pressing enter and waiting)
    sleep 0.3
}

# Thinking pause (variable, 0.5-2s)
think() {
    sleep $(echo "scale=2; 0.5 + $RANDOM % 15 / 10" | bc)
}

# Section header
section() {
    echo ""
    printf "${BOLD}${BLUE}# $1${RESET}\n"
    sleep 0.8
}

# Clear and start
clear
echo -e "${BOLD}rtk - Rust Token Killer${RESET}"
echo -e "${GRAY}Demo: Token-optimized CLI for LLM sessions${RESET}"
sleep 2

# Demo 1: Directory listing
section "Directory listing comparison"
think

type_cmd "rtk ls . -d 2"
rtk ls . -d 2
sleep 2.5

# Demo 2: File reading
section "Smart file reading"
think

type_cmd "rtk read src/main.rs -l aggressive -m 15"
rtk read src/main.rs -l aggressive --max-lines 15
sleep 3

# Demo 3: Grep
section "Compact grep (grouped by file)"
think

type_cmd "rtk grep 'fn run' src/"
rtk grep 'fn run' src/ --max 8
sleep 3

# Demo 4: Git status
section "Git commands"
think

type_cmd "rtk git status"
rtk git status
sleep 2

think
type_cmd "rtk git log -n 5"
rtk git log -n 5
sleep 2.5

# Demo 5: Minimal git operations
section "Minimal output operations"
think

echo -e "${GRAY}# git add/commit/push → just 'ok ✓'${RESET}"
sleep 1
type_cmd "rtk git add --help"
rtk git add --help 2>&1 | head -5
sleep 2

# Demo 6: Dependencies
section "Project analysis"
think

type_cmd "rtk deps"
rtk deps
sleep 2.5

# Summary
echo ""
echo -e "${BOLD}${GREEN}✓ rtk saves 60-90% tokens on common CLI operations${RESET}"
echo -e "${GRAY}  Install: cargo install rtk${RESET}"
echo ""
sleep 3

#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "Usage: $(basename "$0") <pattern> [paths...]" >&2
    echo "Search files in repository using ripgrep when available, otherwise fall back to grep." >&2
    exit 1
fi

PATTERN=$1
shift

if [ "$#" -eq 0 ]; then
    PATHS=(".")
else
    PATHS=("$@")
fi

if command -v rg >/dev/null 2>&1; then
    rg -n --color=always -- "${PATTERN}" "${PATHS[@]}"
else
    if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        git ls-files -z --cached --others --exclude-standard -- "${PATHS[@]}" \
        | xargs -0 grep -n --color=always -E -- "${PATTERN}"
    else
        grep -RIn --color=always --exclude-dir=.git -E -- "${PATTERN}" "${PATHS[@]}"
    fi
fi

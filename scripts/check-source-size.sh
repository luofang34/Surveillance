#!/bin/sh
set -eu

# This gate keeps domain modules small enough to review.
# It names each file that exceeds a limit.
find . \( -path './target' -o -path './.git' \) -prune -o \
    -type f -name '*.rs' -print |
while IFS= read -r source_file; do
    source_lines=$(awk 'END { print NR }' "$source_file")
    if [ "$source_lines" -gt 500 ]; then
        printf '%s\n' "$source_file has $source_lines lines; Rust files allow at most 500" >&2
        exit 1
    fi
    case "$source_file" in
        */lib.rs)
            if [ "$source_lines" -ge 100 ]; then
                printf '%s\n' "$source_file has $source_lines lines; lib.rs must stay below 100" >&2
                exit 1
            fi
            ;;
    esac
done

printf '%s\n' 'Rust source file bounds hold'

#!/bin/sh
set -eu

# A document that names a crate the workspace does not contain sends a reader
# to a path that is not there. This gate compares each crate name in the
# project documents with the crate directories.

status=0
for document in README.md AGENTS.md CLAUDE.md docs/*.md; do
    # A crate name is in code style, or it is a directory in a tree diagram.
    # Other text, for example a key comment, is not a crate reference.
    names=$(grep -oE '`surveillance-[a-z0-9-]+`|surveillance-[a-z0-9-]+/' "$document" \
        | tr -d '`/' | sort -u || true)
    for name in $names; do
        if [ ! -f "crates/$name/Cargo.toml" ]; then
            printf '%s\n' "$document names $name, but crates/$name is not a crate" >&2
            status=1
        fi
    done
done

if [ "$status" -ne 0 ]; then
    exit 1
fi

printf '%s\n' 'documents name only crates that exist'

#!/bin/sh
set -eu

# The engine is source-neutral: it accepts traffic from radios, panels,
# providers, replay, and simulation alike. That only stays true if a radio
# decoder cannot creep back into it, so the boundary is a gate rather than a
# convention.

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

# 1. No dependency edge from the engine to any link decoder, at any depth.
if cargo tree --locked -p surveillance-core --no-default-features --prefix none \
    | grep -qE '^aero-link'; then
    printf '%s\n' 'surveillance-core depends on aero-link:' >&2
    cargo tree --locked -p surveillance-core --no-default-features --invert aero-link >&2 || true
    fail 'the engine must stay source-neutral; put radio translation in surveillance-aero-link'
fi

# 2. No source-level reference either, which catches a path dependency added
#    without a manifest edge and a stray re-export in documentation.
if grep -rn 'aero_link\|aero-link' crates/surveillance-core/src crates/surveillance-core/tests \
    --include='*.rs' >/dev/null 2>&1; then
    printf '%s\n' 'surveillance-core sources mention aero-link:' >&2
    grep -rn 'aero_link\|aero-link' crates/surveillance-core/src crates/surveillance-core/tests \
        --include='*.rs' >&2
    fail 'the engine must name no link decoder; adapters own that vocabulary'
fi

# 3. The umbrella's default build must not pull the adapter in either, so a
#    library consumer never links a decoder they did not ask for.
if cargo tree --locked -p surveillance --no-default-features --prefix none \
    | grep -qE '^(aero-link|surveillance-aero-link)'; then
    fail 'the umbrella default build must not link the aero-link adapter; it is opt-in'
fi

printf '%s\n' 'dependency boundaries hold'

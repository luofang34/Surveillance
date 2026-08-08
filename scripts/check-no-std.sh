#!/bin/sh
set -eu

# Every portable crate builds for the embedded and web targets, adapters
# included: a radio adapter that only builds on a host would quietly make the
# engine unusable on the boards it was written for.
for target in \
    thumbv6m-none-eabi \
    thumbv8m.main-none-eabihf \
    riscv32imac-unknown-none-elf \
    wasm32-unknown-unknown; do
    cargo check --locked \
        -p surveillance-core \
        -p surveillance-aero-link \
        -p surveillance \
        --no-default-features --target "$target"
done

#!/bin/sh
set -eu

for target in \
    thumbv6m-none-eabi \
    thumbv8m.main-none-eabihf \
    riscv32imac-unknown-none-elf \
    wasm32-unknown-unknown; do
    cargo check --locked -p surveillance-core -p surveillance --no-default-features --target "$target"
done

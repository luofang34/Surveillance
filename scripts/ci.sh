#!/bin/sh
set -eu

./scripts/check-boundaries.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
RUSTDOCFLAGS="-D missing_docs -D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps
cargo build --workspace --release

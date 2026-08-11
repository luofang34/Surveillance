#!/bin/sh
set -eu

./scripts/check-boundaries.sh
./scripts/check-source-size.sh
cargo fmt --all -- --check

# The portable build first: it is what a library consumer gets, and it is the
# only pass that compiles the `no_std` half of every conditional.
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

# Then the same gates with every feature on. Host-only code is feature-gated,
# and a gated module and its tests disappear from a default build without
# failing it. Without this pass a gated test reports success because nothing
# compiled it.
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features

RUSTDOCFLAGS="-D missing_docs -D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps
RUSTDOCFLAGS="-D missing_docs -D rustdoc::broken_intra_doc_links" cargo doc --workspace --no-deps --all-features
cargo build --workspace --release

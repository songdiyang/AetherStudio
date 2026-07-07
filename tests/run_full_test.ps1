$env:CARGO_INCREMENTAL = "0"
cargo test --workspace --no-fail-fast 2>&1 | Tee-Object -FilePath "tests/cargo_test_full.log"

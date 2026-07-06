$env:CARGO_INCREMENTAL = "0"
$env:RUSTFLAGS = "-C instrument-coverage"
$env:LLVM_PROFILE_FILE = "tests/coverage/%p-%m.profraw"

$coverageDir = "tests/coverage"
if (Test-Path $coverageDir) {
    Remove-Item -Recurse -Force $coverageDir
}
New-Item -ItemType Directory -Path $coverageDir | Out-Null

cargo test --workspace --no-fail-fast 2>&1 | Tee-Object -FilePath "tests/cargo_test_final.log"

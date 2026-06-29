# Aether Editor Build Script
# 一键构建发布版本

param(
    [switch]$Release,
    [switch]$Run
)

$ErrorActionPreference = "Stop"

Write-Host "=== Aether Editor Build ===" -ForegroundColor Cyan

# 确保Rust环境
$env:Path += ";$env:USERPROFILE\.cargo\bin"

# 构建Rust项目
Write-Host "Building Rust project..." -ForegroundColor Yellow
if ($Release) {
    cargo build --release
} else {
    cargo build
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "Rust build failed!" -ForegroundColor Red
    exit 1
}

$targetDir = if ($Release) { "target\release" } else { "target\debug" }

Write-Host "Build complete!" -ForegroundColor Green
Write-Host "Output: $targetDir\aether-app.exe" -ForegroundColor Green

if ($Run) {
    & "$targetDir\aether-app.exe"
}

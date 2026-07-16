$llvmProfdata = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-profdata.exe"
$llvmCov = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-cov.exe"

$coverageDir = "tests/coverage"
if (Test-Path $coverageDir) {
    Remove-Item -Recurse -Force $coverageDir
}
New-Item -ItemType Directory -Path $coverageDir | Out-Null

# 收集所有 profraw 文件
$profrawFiles = Get-ChildItem -Path . -Filter *.profraw -Recurse -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
if ($profrawFiles.Count -eq 0) {
    Write-Error "No .profraw files found"
    exit 1
}

Write-Host "Found $($profrawFiles.Count) profraw files"
$profrawFiles | ForEach-Object { Write-Host "  $_" }

# 合并
$mergedProfile = "$coverageDir\merged.profdata"
& $llvmProfdata merge -sparse @profrawFiles -o $mergedProfile
if ($LASTEXITCODE -ne 0) {
    Write-Error "llvm-profdata merge failed"
    exit 1
}

# 收集所有测试二进制
$testBinaries = Get-ChildItem -Path "target\x86_64-pc-windows-msvc\debug\deps" -Filter "*.exe" | Where-Object {
    $name = $_.Name
    # 只保留项目 crate 的测试二进制（排除第三方依赖的测试二进制）
    $name -match '^aether(_|-)' -and $name -notmatch '\.\d+\.exe$'
} | Select-Object -ExpandProperty FullName

Write-Host "Found $($testBinaries.Count) test binaries"
$testBinaries | ForEach-Object { Write-Host "  $_" }

# 生成文本报告
$reportArgs = @("report", "--use-color=false", "--instr-profile=$mergedProfile")
foreach ($bin in $testBinaries) {
    $reportArgs += "--object=$bin"
}
$reportArgs += "--ignore-filename-regex=(\.cargo|registry|target)"

& $llvmCov @reportArgs | Tee-Object -FilePath "$coverageDir\coverage_report.txt"

# 生成详细 JSON 报告
$jsonArgs = @("export", "--format=lcov", "--instr-profile=$mergedProfile")
foreach ($bin in $testBinaries) {
    $jsonArgs += "--object=$bin"
}
$jsonArgs += "--ignore-filename-regex=(\.cargo|registry|target)"
& $llvmCov @jsonArgs > "$coverageDir\coverage.lcov"

Write-Host "Coverage report saved to $coverageDir\coverage_report.txt"
Write-Host "LCOV report saved to $coverageDir\coverage.lcov"

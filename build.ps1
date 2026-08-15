# FloatDo 纯 Win32 原生版构建脚本
# 用法：在 PowerShell 中执行 .\build.ps1

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "=== FloatDo Native 构建 ===" -ForegroundColor Cyan

# 1. 检查 Rust 工具链
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "错误：未找到 cargo，请先安装 Rust 工具链 (https://rustup.rs)" -ForegroundColor Red
    exit 1
}

# 2. 编译 release 版（最小体积优化）
Write-Host "正在编译 release 版本..." -ForegroundColor Yellow
cargo build --release

# 3. 输出产物
$exe = Join-Path $PSScriptRoot "target\release\floatdo_native.exe"
if (Test-Path $exe) {
    $size = (Get-Item $exe).Length
    Write-Host ""
    Write-Host "✅ 构建成功！" -ForegroundColor Green
    Write-Host "   产物路径: $exe"
    Write-Host "   文件大小: $([math]::Round($size/1KB,1)) KB ($size 字节)"
    Write-Host ""
    Write-Host "   内存预估: Working Set ~12MB, Private ~1.7MB" -ForegroundColor Gray
} else {
    Write-Host "❌ 构建失败，请检查上方错误信息" -ForegroundColor Red
    exit 1
}

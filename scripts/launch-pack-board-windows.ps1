# Harkonnen Labs — Windows Pack Board launcher
# Starts the Rust API and the Vite Pack Board against the work-windows setup.
#
# Usage (PowerShell, from repo root or anywhere):
#   .\scripts\launch-pack-board-windows.ps1
#   .\scripts\launch-pack-board-windows.ps1 -BackendPort 3057 -FrontendPort 4173 -OpenBrowser

param(
    [int]$BackendPort = 3057,
    [int]$FrontendPort = 4173,
    [string]$Host = "127.0.0.1",
    [switch]$OpenBrowser
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

function Step($msg) { Write-Host "`n>> $msg" -ForegroundColor Cyan }
function Ok($msg)   { Write-Host "   [ok] $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "   [!!] $msg" -ForegroundColor Yellow }
function Fail($msg) { Write-Host "   [xx] $msg" -ForegroundColor Red; exit 1 }

if (-not $env:HARKONNEN_SETUP) {
    $env:HARKONNEN_SETUP = "work-windows"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "cargo is required"
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    Fail "npm is required"
}

$uiDir = Join-Path $RepoRoot "ui"
if (-not (Test-Path (Join-Path $uiDir "package.json"))) {
    Fail "ui/package.json not found"
}

if (-not (Test-Path (Join-Path $uiDir "node_modules"))) {
    Step "Installing UI dependencies"
    Push-Location $uiDir
    try {
        npm install
        if ($LASTEXITCODE -ne 0) {
            Fail "npm install failed for ui/"
        }
    }
    finally {
        Pop-Location
    }
    Ok "UI dependencies installed"
}

$apiLog = Join-Path $env:TEMP "harkonnen-pack-board-api.log"
$apiErr = Join-Path $env:TEMP "harkonnen-pack-board-api.err.log"

Step "Starting Harkonnen API on http://127.0.0.1:$BackendPort"
$backend = Start-Process -FilePath "cargo" -ArgumentList @("run", "--", "serve", "--port", "$BackendPort") -WorkingDirectory $RepoRoot -RedirectStandardOutput $apiLog -RedirectStandardError $apiErr -PassThru

Start-Sleep -Seconds 3
if ($backend.HasExited) {
    Write-Host "--- API stdout ---"
    if (Test-Path $apiLog) { Get-Content $apiLog }
    Write-Host "--- API stderr ---"
    if (Test-Path $apiErr) { Get-Content $apiErr }
    Fail "backend failed to start"
}
Ok "API pid=$($backend.Id) log=$apiLog"

$env:VITE_API_BASE = "http://127.0.0.1:$BackendPort/api"
$frontendUrl = "http://$Host:$FrontendPort"

if ($OpenBrowser) {
    Start-Process $frontendUrl | Out-Null
}

Write-Host ""
Write-Host "Primary interfaces:"
Write-Host "  Claude Code in your target repo  -> where you talk to the Labradors"
Write-Host "  Pack Board UI                    -> $frontendUrl"
Write-Host "  Factory API                      -> http://127.0.0.1:$BackendPort/api"
Write-Host ""
Write-Host "API logs:"
Write-Host "  stdout: $apiLog"
Write-Host "  stderr: $apiErr"
Write-Host ""
Write-Host "Press Ctrl+C in this terminal to stop the frontend."
Write-Host "Stop the backend with: Stop-Process -Id $($backend.Id)"

try {
    Set-Location $uiDir
    npm run dev -- --host $Host --port $FrontendPort
}
finally {
    if ($backend -and -not $backend.HasExited) {
        Stop-Process -Id $backend.Id -Force
    }
}

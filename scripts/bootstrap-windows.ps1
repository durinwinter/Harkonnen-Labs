# Harkonnen Labs - Windows bootstrap
# Sets up the work-windows setup: Claude only, MCP servers via npx, no Docker.
#
# Usage (run from repo root in an elevated PowerShell terminal):
#   .\scripts\bootstrap-windows.ps1
#
# Optional: pass a custom base directory
#   .\scripts\bootstrap-windows.ps1 -BaseDir "C:\harkonnen-local"

param(
    [string]$BaseDir = "$env:USERPROFILE\harkonnen-local"
)

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "Harkonnen Labs - Windows Bootstrap"
Write-Host "==================================="
Write-Host "Base dir: $BaseDir"
Write-Host ""

# -- Prerequisites check -------------------------------------------------------

$missing = @()

if (-not (Get-Command node   -ErrorAction SilentlyContinue)) { $missing += "node  (https://nodejs.org)" }
if (-not (Get-Command npm    -ErrorAction SilentlyContinue)) { $missing += "npm   (bundled with Node.js)" }
if (-not (Get-Command cargo  -ErrorAction SilentlyContinue)) { $missing += "cargo (https://rustup.rs)" }

if ($missing.Count -gt 0) {
    Write-Host "ERROR: Missing prerequisites:"
    $missing | ForEach-Object { Write-Host "  - $_" }
    Write-Host ""
    Write-Host "Install the above, then re-run this script."
    exit 1
}

Write-Host "Prerequisites: ok"
Write-Host "  node  $(node  --version)"
Write-Host "  npm   $(npm   --version)"
Write-Host "  cargo $(cargo --version)"
Write-Host ""

# -- Directories ---------------------------------------------------------------

$dirs = @(
    $BaseDir,
    "factory\specs",
    "factory\scenarios",
    "factory\artifacts",
    "factory\workspaces",
    "factory\memory",
    "factory\logs",
    "products"
)

foreach ($d in $dirs) {
    $target = if ([System.IO.Path]::IsPathRooted($d)) { $d } else { Join-Path (Get-Location) $d }
    New-Item -ItemType Directory -Force -Path $target | Out-Null
}

Write-Host "Directories: ok"
Write-Host ""

# -- MCP servers (install globally so npx -y works offline) -------------------

Write-Host "Installing MCP servers..."

$mcpPackages = @(
    "@modelcontextprotocol/server-filesystem",
    "@modelcontextprotocol/server-memory",
    "@modelcontextprotocol/server-sqlite",
    "@modelcontextprotocol/server-github",
    "@modelcontextprotocol/server-brave-search"
)

npm install --global @($mcpPackages)

Write-Host ""
Write-Host "MCP servers: ok"
Write-Host ""

# -- Environment file ----------------------------------------------------------

$envTarget = Join-Path (Get-Location) ".env"

if (-not (Test-Path $envTarget)) {
    $envSource = Join-Path (Get-Location) ".env.example"
    if (Test-Path $envSource) {
        Copy-Item $envSource $envTarget
        Write-Host "Created .env from .env.example"
    }
} else {
    Write-Host ".env already exists - skipping"
}

Write-Host ""

# -- Summary -------------------------------------------------------------------

Write-Host "Bootstrap complete."
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Edit .env and set ANTHROPIC_API_KEY=sk-ant-..."
Write-Host "     (and HARKONNEN_SETUP=work-windows)"
Write-Host ""
Write-Host "  2. Verify setup:"
Write-Host "     cargo run -- setup check"
Write-Host ""
Write-Host "  3. Validate a spec:"
Write-Host "     cargo run -- spec validate factory\specs\examples\sample_feature.yaml"
Write-Host ""

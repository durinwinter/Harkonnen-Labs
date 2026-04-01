# Harkonnen Labs - Windows Factory Bring-Up
# Single script to go from fresh clone to a running factory spine on work-windows.
#
# Run from the repo root in an elevated PowerShell terminal:
#   .\scripts\factory-up-windows.ps1
#
# Idempotent - safe to re-run. Existing .env and seed docs are not overwritten.
#
# What this script does:
#   1.  Check prerequisites (node, npm, cargo)
#   2.  Resolve ANTHROPIC_API_KEY (prompt if missing)
#   3.  Write .env (if absent)
#   4.  Set HARKONNEN_SETUP=work-windows for this session
#   5.  Install MCP server npm packages
#   6.  Build the harkonnen binary (cargo build)
#   7.  Run: harkonnen memory init  (seed Coobie, build index)
#   8.  Run: harkonnen setup check  (verify providers + MCP)
#   9.  Write .claude/settings.local.json  MCP block
#   10. Print current factory status and next steps, including Claude pack export

param(
    [switch]$Release   # pass -Release to build with optimizations
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

# -- Helpers -------------------------------------------------------------------

function Step($msg) { Write-Host "`n  >> $msg" -ForegroundColor Cyan }
function Ok($msg)   { Write-Host "     [ok] $msg"      -ForegroundColor Green }
function Warn($msg) { Write-Host "     [!!] $msg"      -ForegroundColor Yellow }
function Fail($msg) { Write-Host "     [XX] $msg`n"    -ForegroundColor Red; exit 1 }
function Info($msg) { Write-Host "          $msg" }

function Require($cmd, $hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Fail "Missing: $cmd  ->  $hint"
    }
    Ok "$cmd $(& $cmd --version 2>&1 | Select-Object -First 1)"
}

function Require-NodeVersion($min) {
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Fail "Node.js is required (v$min+). Install from https://nodejs.org"
    }
    $versionText = (& node --version 2>&1 | Select-Object -First 1).ToString().Trim()
    $majorText = $versionText.TrimStart('v', 'V').Split('.')[0]
    $major = 0
    if (-not [int]::TryParse($majorText, [ref]$major)) {
        Fail "Could not parse Node.js version from: $versionText"
    }
    if ($major -lt $min) {
        Fail "Node.js v$min+ required (found: $versionText). Upgrade at https://nodejs.org"
    }
    Ok "node $versionText - meets minimum v$min"
}

function Test-PortFree($port) {
    $connections = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()
    $taken = $connections | Where-Object { $_.Port -eq $port }
    return ($null -eq $taken -or $taken.Count -eq 0)
}

function Check-Port($port, $label) {
    if (Test-PortFree $port) {
        Ok "Port $port is free ($label)"
    } else {
        Warn "Port $port is already in use ($label)"
        Info "Find the process: netstat -ano | findstr :$port"
        # Return false so caller can decide whether to fail
        return $false
    }
    return $true
}

function Npm-GlobalInstall($pkg) {
    $output = npm list -g --depth=0 $pkg 2>&1
    if ($LASTEXITCODE -eq 0) {
        Ok "$pkg (already installed)"
        return
    }
    Info "Installing $pkg ..."
    $output = npm install --global $pkg 2>&1
    if ($LASTEXITCODE -ne 0) {
        Warn "npm install -g $pkg failed (exit $LASTEXITCODE)"
        $output | Select-Object -First 5 | ForEach-Object { Info $_.ToString() }
        Info "Try running this terminal as Administrator, or set a user-writable prefix:"
        Info "  npm config set prefix `"$env:APPDATA\npm`""
        $script:McpFailures++
    } else {
        Ok "$pkg installed"
    }
}

# -- Banner --------------------------------------------------------------------

Write-Host ""
Write-Host "  +==============================================+" -ForegroundColor DarkCyan
Write-Host "  |   Harkonnen Labs - Windows Factory Bring-Up  |" -ForegroundColor DarkCyan
Write-Host "  +==============================================+" -ForegroundColor DarkCyan
Write-Host "  Setup: work-windows (Claude only, no Docker)"
Write-Host "  Root:  $RepoRoot"

# -- 1. Prerequisites ----------------------------------------------------------

Step "Checking prerequisites"
Require-NodeVersion 18
Require "npm"   "bundled with Node.js"
Require "cargo" "https://rustup.rs"

# Port check for the harkonnen serve port (3000). Informational - does not block install.
Check-Port 3000 "harkonnen serve" | Out-Null

# -- 2. ANTHROPIC_API_KEY ------------------------------------------------------

Step "Resolving ANTHROPIC_API_KEY"

# Load .env if it exists
$EnvFile = Join-Path $RepoRoot ".env"
if (Test-Path $EnvFile) {
    Get-Content $EnvFile | ForEach-Object {
        $line = $_.ToString()
        if ([string]::IsNullOrWhiteSpace($line)) { return }
        $trimmed = $line.Trim()
        if ($trimmed.StartsWith('#')) { return }
        $separator = $line.IndexOf('=')
        if ($separator -lt 1) { return }
        $k = $line.Substring(0, $separator).Trim()
        $v = $line.Substring($separator + 1).Trim()
        if (-not [string]::IsNullOrWhiteSpace($k) -and -not [System.Environment]::GetEnvironmentVariable($k)) {
            [System.Environment]::SetEnvironmentVariable($k, $v, 'Process')
        }
    }
    Info "Loaded $EnvFile"
}

if (-not $env:ANTHROPIC_API_KEY) {
    Warn "ANTHROPIC_API_KEY is not set."
    $key = Read-Host "  Enter your Anthropic API key (sk-ant-...) or press Enter to skip"
    if ($key) {
        $env:ANTHROPIC_API_KEY = $key.Trim()
        Ok "Key accepted for this session"
    } else {
        Warn "Skipping - setup check will report MISSING until you set ANTHROPIC_API_KEY"
    }
} else {
    Ok "ANTHROPIC_API_KEY is set"
}

# -- 3. Write .env -------------------------------------------------------------

Step "Writing .env"

if (-not (Test-Path $EnvFile)) {
    $example = Join-Path $RepoRoot ".env.example"
    if (Test-Path $example) {
        Copy-Item $example $EnvFile
        Ok "Created .env from .env.example"
    } else {
        Set-Content $EnvFile "HARKONNEN_SETUP=work-windows`nANTHROPIC_API_KEY="
        Ok "Created minimal .env"
    }
} else {
    Ok ".env already exists - not overwritten"
}

# Ensure HARKONNEN_SETUP is in the file
$envContent = Get-Content $EnvFile
$hasSetup = $false
foreach ($line in $envContent) {
    $trimmed = $line.ToString().TrimStart()
    if ($trimmed.StartsWith('HARKONNEN_SETUP=')) {
        $hasSetup = $true
        break
    }
}
if (-not $hasSetup) {
    Add-Content $EnvFile "`nHARKONNEN_SETUP=work-windows"
    Info "Added HARKONNEN_SETUP=work-windows to .env"
}

# -- 4. Session environment ----------------------------------------------------

Step "Setting session environment"
$env:HARKONNEN_SETUP = "work-windows"
Ok "HARKONNEN_SETUP=work-windows"

# -- 5. Install MCP packages ---------------------------------------------------

Step "Installing MCP server packages (npm install -g)"

$packages = @(
    "@modelcontextprotocol/server-filesystem",
    "@modelcontextprotocol/server-memory",
    "@modelcontextprotocol/server-sqlite"
)

$script:McpFailures = 0
foreach ($pkg in $packages) {
    Npm-GlobalInstall $pkg
}

if ($script:McpFailures -gt 0) {
    Warn "$($script:McpFailures) MCP package(s) failed to install - Claude Code MCP tools will not work until resolved"
}

# -- 6. Build harkonnen binary -------------------------------------------------

Step "Building harkonnen binary"

if ($Release) {
    Info "cargo build --release"
    cargo build --release 2>&1 | Where-Object { $_ -notmatch "^$" } | ForEach-Object { Info $_ }
    $BinPath = Join-Path $RepoRoot "target\release\harkonnen.exe"
} else {
    Info "cargo build  (pass -Release for an optimized build)"
    cargo build 2>&1 | Where-Object { $_ -notmatch "^$" } | ForEach-Object { Info $_ }
    $BinPath = Join-Path $RepoRoot "target\debug\harkonnen.exe"
}

if (-not (Test-Path $BinPath)) {
    Fail "Build failed - harkonnen.exe not found at $BinPath"
}
Ok "Binary: $BinPath"

# -- Helper to run harkonnen ---------------------------------------------------

function Harkonnen {
    param([string[]]$Args)
    & $BinPath @Args
    if ($LASTEXITCODE -ne 0) {
        Fail "harkonnen $($Args -join ' ') exited with code $LASTEXITCODE"
    }
}

# -- 7. Memory init (Coobie) ---------------------------------------------------

Step "Initializing Coobie's memory  (harkonnen memory init)"

Harkonnen "memory", "init"

# -- 8. Setup check ------------------------------------------------------------

Step "Verifying setup  (harkonnen setup check)"

Write-Host ""
Harkonnen "setup", "check"
Write-Host ""

# -- 9. Write Claude Code MCP config ------------------------------------------

Step "Writing .claude/settings.local.json (MCP server config)"

$ClaudeDir     = Join-Path $RepoRoot ".claude"
$ClaudeSettings = Join-Path $ClaudeDir "settings.local.json"

New-Item -ItemType Directory -Force -Path $ClaudeDir | Out-Null

$McpBlock = [ordered]@{
    mcpServers = [ordered]@{
        memory = [ordered]@{
            command = "npx"
            args    = @("-y", "@modelcontextprotocol/server-memory")
            env     = [ordered]@{ MEMORY_FILE_PATH = "./factory/memory/store.json" }
        }
        filesystem = [ordered]@{
            command = "npx"
            args    = @(
                "-y", "@modelcontextprotocol/server-filesystem",
                "./products",
                "./factory/workspaces",
                "./factory/artifacts",
                "./factory/memory"
            )
        }
        sqlite = [ordered]@{
            command = "npx"
            args    = @("-y", "@modelcontextprotocol/server-sqlite", "./factory/state.db")
        }
    }
}

if (Test-Path $ClaudeSettings) {
    # Merge into existing file - guard against malformed JSON
    $rawJson = Get-Content $ClaudeSettings -Raw
    $existing = $null
    try {
        $existing = $rawJson | ConvertFrom-Json
    } catch {
        Warn "settings.local.json exists but could not be parsed: $_"
        $backup = "$ClaudeSettings.bak"
        Copy-Item $ClaudeSettings $backup -Force
        Warn "Backed up to $backup - will overwrite with a fresh config"
    }

    if ($null -ne $existing) {
        if (-not $existing.PSObject.Properties['mcpServers']) {
            $existing | Add-Member -NotePropertyName mcpServers -NotePropertyValue $McpBlock.mcpServers
        } else {
            foreach ($key in $McpBlock.mcpServers.Keys) {
                if ($existing.mcpServers.PSObject.Properties[$key]) {
                    $existing.mcpServers.PSObject.Properties.Remove($key)
                }
                $existing.mcpServers | Add-Member -NotePropertyName $key `
                    -NotePropertyValue $McpBlock.mcpServers[$key]
            }
        }
        $existing | ConvertTo-Json -Depth 10 | Set-Content $ClaudeSettings -Encoding UTF8
        Ok "Merged MCP servers into existing $ClaudeSettings"
    } else {
        $McpBlock | ConvertTo-Json -Depth 10 | Set-Content $ClaudeSettings -Encoding UTF8
        Ok "Created fresh $ClaudeSettings (old file backed up)"
    }
} else {
    $McpBlock | ConvertTo-Json -Depth 10 | Set-Content $ClaudeSettings -Encoding UTF8
    Ok "Created $ClaudeSettings"
}

# -- 10. Summary ---------------------------------------------------------------

Write-Host ""
Write-Host "  ================================================" -ForegroundColor DarkCyan
Write-Host "  Factory spine is up." -ForegroundColor Green
Write-Host "  ================================================" -ForegroundColor DarkCyan
Write-Host ""

Write-Host "  What is working now:" -ForegroundColor White
@(
    "harkonnen spec validate <file>          - Scout: parse and validate specs",
    "harkonnen run start <spec> --product X  - create a run record + workspace",
    "harkonnen run status <id>               - check run status",
    "harkonnen run report <id>               - print run report",
    "harkonnen artifact package <id>         - package artifacts",
    "harkonnen memory init / index           - Coobie: seed + rebuild memory index",
    "harkonnen setup check                   - verify providers + MCP servers",
    "MCP: memory, filesystem, sqlite         - available to Claude Code"
) | ForEach-Object { Write-Host "    [+] $_" -ForegroundColor Green }

Write-Host ""
Write-Host ""
Write-Host "  Working now, beyond the factory spine:" -ForegroundColor White
@(
    "Provider-aware LLM routing  - Scout, Mason, Piper, Bramble, and Ash can call configured providers with fallback paths",
    "Hidden scenarios            - protected scenario files are evaluated through the Rust run pipeline",
    "Digital twin manifests      - Ash provisions a safe local twin manifest with dependency stubs and optional narrative",
    "Coordination API            - Keeper file claims, heartbeats, and conflict policy are available through harkonnen serve",
    "Pack Board UI               - React/Vite UI and API-backed run detail views exist in the repo",
    "Claude pack export          - setup claude-pack can stamp another repo with Labradors, context, and MCP wiring"
) | ForEach-Object { Write-Host "    [+] $_" -ForegroundColor Green }

Write-Host ""
Write-Host "  Still optional or still evolving:" -ForegroundColor White
@(
    "Direct Claude cowork spawning from the Rust orchestrator is not wired yet - the Claude pack exporter is the handoff path today",
    "Richer black-box hidden scenarios beyond artifact/event checks can still be expanded",
    "WinCC OA MCP integration depends on the target machine and project environment",
    "Coobie semantic memory and DeepCausality phase 2 remain future upgrades"
) | ForEach-Object { Write-Host "    [ ] $_" -ForegroundColor DarkGray }

Write-Host ""
Write-Host "  Next steps:" -ForegroundColor White
Write-Host "    1. Restart Claude Code - it will pick up the new MCP servers"
Write-Host "    2. Validate the factory: harkonnen spec validate factory\specs\examples\sample_feature.yaml"
Write-Host '    3. Export a Labrador pack into SPO: harkonnen setup claude-pack --target-path <path-to-SPO> --project-name SPO --project-type winccoa --domain "Siemens WinCC OA / industrial automation" --summary "SPO is a WinCC OA based Siemens product operated through a Claude-only Labrador pack." --winccoa'
Write-Host "    4. Launch the Pack Board: .\scripts\launch-pack-board-windows.ps1 -OpenBrowser"
Write-Host "    5. Open the target repo in Claude Code, run /agents, and start with Scout"
Write-Host ""
Write-Host "  Primary interface: Claude Code in the stamped target repo"
Write-Host "  Control room:       Pack Board (browser UI + API)"
Write-Host ""
Write-Host "  Binary:  $BinPath"
Write-Host "  Setup:   $env:HARKONNEN_SETUP"
Write-Host "  Memory:  $RepoRoot\factory\memory\"
Write-Host ""

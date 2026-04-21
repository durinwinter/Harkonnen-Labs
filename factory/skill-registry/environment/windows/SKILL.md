---
name: windows
description: "Windows environment conventions: PowerShell idioms, service management, path conventions, and winget usage for this repo."
user-invocable: false
allowed-tools:
  - Bash(powershell *)
  - Bash(pwsh *)
  - Bash(sc *)
  - Bash(reg *)
---

# Windows Environment Guide

This repo runs on Windows. Apply these conventions.

## Shell

- Prefer PowerShell (`pwsh`) over `cmd.exe` for scripting.
- Use `$env:VARNAME` for environment variables in PowerShell.
- For batch scripts, use `%VARNAME%`; avoid mixing styles.
- Paths use backslash `\` on Windows — use `Join-Path` in PowerShell instead of string concatenation.

## Paths

- User home: `$env:USERPROFILE` or `[Environment]::GetFolderPath('UserProfile')`.
- App data: `$env:APPDATA` (roaming), `$env:LOCALAPPDATA` (local).
- Program files: `$env:ProgramFiles`, `$env:ProgramFiles(x86)`.
- Never hardcode drive letters or usernames.

## Package Management

- System packages: `winget install --id Publisher.Package --exact`
- Chocolatey if available: `choco install package -y`
- Pin versions where reproducibility matters.

## Services

- Check status: `Get-Service -Name ServiceName` or `sc query ServiceName`
- Start/stop: `Start-Service`, `Stop-Service`
- Never stop a service without checking if it is production-facing.
- Event log: `Get-EventLog -LogName Application -Source ServiceName -Newest 20`

## PowerShell Conventions

- Use `$ErrorActionPreference = 'Stop'` at the top of critical scripts.
- Test paths before accessing: `Test-Path $path` before `Get-Content $path`.
- Use `Write-Host` sparingly; prefer `Write-Output` for pipeline-compatible output.
- Avoid aliases in scripts (`ls`, `cat`, `echo`) — use full cmdlet names.

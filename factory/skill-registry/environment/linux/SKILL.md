---
name: linux
description: "Linux environment conventions: shell idioms, systemd, package management, file permissions, and path conventions for this repo."
user-invocable: false
allowed-tools:
  - Bash(ls *)
  - Bash(systemctl *)
  - Bash(journalctl *)
  - Bash(apt *)
  - Bash(apt-get *)
  - Bash(dpkg *)
  - Bash(dnf *)
  - Bash(rpm *)
---

# Linux Environment Guide

This repo runs on Linux. Apply these conventions.

## Shell

- Use bash unless the script explicitly declares another shell.
- Prefer `#!/usr/bin/env bash` over hardcoded `/bin/bash`.
- Quote all variable expansions: `"$VAR"`, not `$VAR`.
- Use `set -euo pipefail` in scripts that must not silently fail.

## Paths

- Home: `$HOME` or `~` — never hardcode `/home/user`.
- Config: `$XDG_CONFIG_HOME` (default `~/.config`) for user-scoped settings.
- Data: `$XDG_DATA_HOME` (default `~/.local/share`).
- Binaries: check `/usr/local/bin` before `/usr/bin`; install to `/usr/local/bin` for local installs.

## Packages

- Prefer the system package manager (`apt`/`dnf`/`pacman`) for system-level installs.
- Use `apt-get` in scripts; `apt` in interactive sessions.
- Pin versions in install scripts to avoid silent drift: `apt-get install -y pkg=version`.

## Services

- Check service status: `systemctl status <service>`
- View logs: `journalctl -u <service> -n 50 --no-pager`
- Never restart a service without checking whether it is in production use first.

## File Permissions

- Scripts must be executable: `chmod +x script.sh`
- Config files with secrets: `chmod 600`; directories containing them: `chmod 700`.
- Avoid `chmod 777` — use group permissions instead.

## Processes

- `ps aux | grep <name>` to confirm a process is running.
- `kill -0 <pid>` to check if a PID exists without sending a signal.
- Prefer `pkill -f pattern` over guessing PIDs.

---
name: macos
description: "macOS environment conventions: Homebrew, launchd, shell idioms, and path conventions for this repo."
user-invocable: false
allowed-tools:
  - Bash(brew *)
  - Bash(launchctl *)
---

# macOS Environment Guide

This repo runs on macOS. Apply these conventions.

## Shell

- Default shell is zsh since macOS 10.15; scripts should declare `#!/usr/bin/env zsh` or `#!/usr/bin/env bash`.
- Quote all variable expansions.
- `set -euo pipefail` for safety in scripts.

## Paths

- Home: `$HOME`
- Config: `~/.config` (XDG) or `~/Library/Application Support` for GUI apps.
- Binaries (Homebrew): `/opt/homebrew/bin` (Apple Silicon) or `/usr/local/bin` (Intel).
- Always add Homebrew prefix to PATH in scripts: `eval "$(brew shellenv)"`.

## Package Management

- Use Homebrew: `brew install package`
- Cask for GUI apps: `brew install --cask app-name`
- Pin versions: `brew pin package` to prevent upgrades during CI.

## Services

- launchd manages background services: `launchctl load/unload ~/Library/LaunchAgents/com.example.plist`
- Check status: `launchctl list | grep com.example`
- Never touch system-level launch daemons without root and explicit approval.

## Filesystem Notes

- macOS has case-insensitive (but case-preserving) filesystem by default — watch for path case bugs when porting from Linux.
- Extended attributes (xattr) can block scripts: `xattr -d com.apple.quarantine file` if needed.

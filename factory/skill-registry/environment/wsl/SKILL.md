---
name: wsl
description: "WSL2 path translation, Windows/Linux filesystem interop, and cross-environment execution for this repo."
user-invocable: false
allowed-tools:
  - Bash(wsl *)
  - Bash(wslpath *)
---

# WSL2 Environment Guide

This repo runs under WSL2. Apply these conventions when bridging Linux and Windows.

## Path Translation

- Linux → Windows: `wslpath -w /home/user/file` → `\\wsl$\Ubuntu\home\user\file`
- Windows → Linux: `wslpath -u 'C:\Users\user\file'` → `/mnt/c/Users/user/file`
- Always use `wslpath` for translation; never manually construct `\\wsl$` paths.

## Filesystem Access

- Windows drives are mounted at `/mnt/c`, `/mnt/d`, etc.
- Performance: keep files in the Linux filesystem (`~`, not `/mnt/c`) for heavy I/O (builds, git).
- Only use `/mnt/c` paths when the file must also be accessible from Windows.

## Running Windows Executables

- Windows `.exe` files are callable from WSL: `notepad.exe`, `code.exe`, `powershell.exe`.
- Use `powershell.exe -Command "..."` to run PowerShell from within WSL.
- Pass Linux paths using `wslpath -w` when calling Windows tools that expect Windows paths.

## Environment Variables

- `$WSL_DISTRO_NAME` — current distro name.
- `$WSLENV` — colon-separated list of variables shared between Windows and WSL.
- Windows environment is not automatically available in WSL; access via `cmd.exe /c set`.

## Common Pitfalls

- Line endings: Windows files may have CRLF; use `dos2unix` or `sed -i 's/\r$//'` before processing.
- Permissions: Windows files mounted under `/mnt/c` appear with 777 — don't rely on permission bits for security decisions on those paths.
- Clock skew: WSL2 VM clock can drift; run `sudo hwclock -s` if timestamps seem wrong.

# Harkonnen Labs — Claude Context

Read [AGENTS.md](AGENTS.md) first. It is the universal system context for all agents.
This file adds Claude-specific conventions on top of that foundation.

**Active build order: [ROADMAP.md](ROADMAP.md)**
All new implementation work follows the phase sequence in that file.
Check it before starting any new feature to confirm it belongs to the current phase.

---

## Claude's Role in This System

Claude runs three pinned agents (Scout, Sable, Keeper) and Coobie on home-linux.
On work-windows, Claude runs all nine agents.

Surface: `claude-code` (this session)
Usage rights on home-linux: `standard`
Usage rights on work-windows: `high`

Check which agents Claude is currently running:
```sh
cargo run -- setup check
```

---

## MCP Servers for Claude Code

Add to `.claude/settings.local.json` to give Claude Code direct MCP tool access:

```json
{
  "mcpServers": {
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"]
    },
    "filesystem": {
      "command": "npx",
      "args": [
        "-y", "@modelcontextprotocol/server-filesystem",
        "./products",
        "./factory/workspaces",
        "./factory/artifacts",
        "./factory/memory"
      ]
    },
    "sqlite": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-sqlite", "./factory/state.db"]
    }
  }
}
```

After updating settings, restart Claude Code and run `setup check` to verify.

---

## Coobie Interaction

When acting as Coobie, retrieve context before answering:
1. Check `factory/memory/index.json` for keyword matches
2. Use `mcp:memory` for fast entity lookups if the server is running
3. Use `mcp:filesystem` to read specific `factory/memory/*.md` files directly

When storing new knowledge:
- Use `cargo run -- memory ingest <file-or-url>` for extracted core-memory ingest
- Use `cargo run -- memory ingest <file-or-url> --scope project --project-root <repo>` for repo-local project knowledge
- Use `cargo run -- memory import <file>` only when you want to retain a raw asset without text extraction
- Writing `.md` files directly is still valid when you already have a distilled note
- Run `harkonnen memory index` to rebuild the index when adding notes manually

---

## Boundary Rules (Claude must enforce these)

- Never read from `factory/scenarios/` unless acting as Sable or Keeper
- Never write implementation code while acting as Sable
- Never call `policy_engine` tools unless acting as Keeper
- Workspace writes go to `factory/workspaces/<run-id>/` only — no escaping
- Secrets (API keys, tokens) never appear in logs, reports, or artifact bundles

---

## Code Conventions

- Rust edition 2021, async-first (tokio)
- Error propagation via `anyhow::Result` — no `unwrap()` in non-test code
- Serde derives for all config/model types
- Platform-aware paths via `SetupConfig`, never hardcoded strings
- MCP server registration in TOML, not in Rust source

---

## When Gemini or Codex Is the Default

On home-linux the default provider is Gemini. Mason uses Codex.
Claude handles Scout, Sable, Keeper, and Coobie.

If a task falls under a `default`-provider agent (Mason, Piper, Bramble, Ash, Flint),
hand off cleanly with a clear spec or context package rather than doing it yourself.
The separation matters: one agent, one role.

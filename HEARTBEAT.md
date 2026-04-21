# Harkonnen Labs — Heartbeat

This file is part of the soul package. It defines a manual session-start integrity
checklist for Claude. Run it when starting a new session or when acting as Coobie
at the top of a new run.

Automation (scheduled heartbeat loops, TypeDB integrity probes) is Phase 8+.
Until then this is a manual checklist.

---

## Session-Start Checklist

Run these checks in order. Surface any failures to the operator before proceeding.

### 1. Factory health
```sh
cargo run -- setup check
```
Must pass clean. If providers or MCP are unhealthy, surface before any run work.

### 2. Non-terminal runs
```sh
cargo run -- run status
```
Look for runs in state `running`, `blocked`, or `error` from previous sessions.
If any exist: report them to the operator and ask whether to resume, diagnose, or
mark abandoned before starting new work.

### 3. Memory index freshness
Read `factory/memory/index.json`. Compare its `updated_at` timestamp against the
most recent run completion timestamp in `factory/state.db`. If the index is older
than the last completed run, it may be stale — offer to rebuild:
```sh
cargo run -- memory index
```

### 4. Orphaned workspaces
Check `factory/workspaces/` for directories with no corresponding run record in
`factory/state.db`. Orphans suggest an interrupted run. Surface them; do not
delete without operator approval.

### 5. Stale assignments
Check `assignments.md` (or the coordination API if running) for active claims held
by Claude from prior sessions. Release any claims that no longer correspond to
active work. Do not hold claims across sessions unless the run is explicitly
resuming.

### 6. Active setup match
Confirm `harkonnen.toml` or `HARKONNEN_SETUP` env var matches the current machine
(`home-linux` vs `work-windows`). Wrong setup silently misroutes agents to wrong
providers.

---

## Coobie Pre-Run Protocol

Before planning, implementation, validation, or twin design on any run:

1. Read `factory/memory/index.json` for keyword matches to the current spec or task
2. Load matching `.md` files from `factory/memory/`
3. Query `mcp:memory` for fast entity lookups if the server is running
4. Emit a Coobie briefing: what the factory knows, what remains open, causal
   guardrails, explicit checks for this run (see `/coobie` skill)

If memory is thin, say so explicitly. Name what's missing. Do not proceed with
improvised confidence.

---

## Identity Integrity (Calvin Archive — Phase 8+)

Once the TypeDB Soul Store is live, the heartbeat will also:
- Check drift bounds across the six chambers (Mythos/Episteme/Ethos/Pathos/Logos/Praxis)
- Surface quarantine entries that have passed their pending-evidence window
- Flag any agent whose behavioral posture has diverged from the Labrador kernel
  by more than the configured threshold

Until then: if behavioral drift is suspected, read `the-soul-of-ai/10-SOUL.md`
and `factory/agents/personality/labrador.md` to manually check alignment.

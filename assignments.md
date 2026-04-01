# Assignments

This is the fallback coordination document when the Harkonnen API server is not running.

Keeper manages file-claim policy for this repo.
Policy mode: exclusive_file_claims
Heartbeat timeout: 600 seconds

Preferred live source once the server is up: `GET /api/coordination/assignments`.

Policy event stream: `GET /api/coordination/policy-events`.

Claim work with `POST /api/coordination/claim`, heartbeat with `POST /api/coordination/heartbeat`, and release it with `POST /api/coordination/release`.

Last updated: 2026-04-01T00:24:30.576399326+00:00

## Active Claims

### codex
Task: organize lamdet example as harkonnen run pattern
Status: stale
Claimed: 2026-04-01T00:14:29.237977040+00:00
Last heartbeat: 2026-04-01T00:14:29.237977040+00:00
Files:
- src/models.rs
- src/spec.rs
- src/orchestrator.rs
- src/scenarios.rs
- factory/specs
- factory/context
- factory/memory

## How To Use This Fallback

1. Before assigning work, read the relevant active claim section.
2. Paste only the relevant section into the AI's context.
3. If you are actively holding files, send a heartbeat about once per minute.
4. Keeper may reap stale conflicting claims when another agent needs the same files.
5. Once the server is running, switch all agents to the live coordination endpoint.

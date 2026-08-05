# Phase 6 TypeDB Adapter — Deferred Follow-Ups

Recorded 2026-08-02, at the close of `docs/superpowers/plans/2026-08-01-phase6-typedb-adapter.md`
(branch `worktree-phase6-typedb-adapter`, commits `b5ed657..7e4dd15`).

Every item below was raised by a review during that work, triaged as
non-blocking, and deliberately deferred. None of them blocks merge. They are
recorded here because the working ledger they came from is git-ignored scratch.

## Correctness / robustness

1. **Exported replay queries are stale TypeQL 2.x.** `src/api.rs` (~`:3804`,
   `:3811`, `:3824`) emits TypeQL using `get $e, $o, $status;` and role names
   that no longer exist after Task 2 rewrote the schema: it uses
   `(goal: …, episode: …) isa episode-goal` (real roles: `target-goal` /
   `episode-context`), `(episode: …, outcome: …) isa produced-outcome`
   (real role: `episode-context`), and `(episode: …, failure: …) isa
   classifies-failure` (real roles: `failure` / `outcome`). Pre-existing and
   outside this plan's scope, but Phase 6 makes the schema live, so these are
   now demonstrably-broken queries handed to operators in a markdown artifact.
   Fix them or mark them known-stale.

2. **`schema_is_present()` can trigger a pointless redeploy.** A transient
   error (anything other than "type not found") returns `Ok(false)`, so
   `connect()` re-runs `SCHEMA_TQL` against a database that already has those
   types. `schema.tql` has no if-not-exists guard, so that `connect()` errors
   and falls back to Noop. Narrow, self-heals on the next connect, and stays
   inside the accepted degrade-to-Noop envelope.

3. **Two orphaned liveness probes can briefly overlap.** Under sustained
   failure the probe cadence (`PING_CACHE_TTL + PING_TIMEOUT` ≈ 1.75s) is
   slightly shorter than the driver's ~2s internal blocking retry, so
   steady-state is ≤2 orphans rather than a strict 1. Bounded; blocking pool
   max is 512; flood testing showed flat thread counts.

4. **`CONNECT_TIMEOUT` (10s) serves two roles** — both the per-RPC
   `request_timeout` and the whole-`connect()` budget. Neither is wrong today;
   worth revisiting now that `QUERY_TIMEOUT` exists and the constants form a
   family.

## Observability / accuracy

5. **Failure-path notes omit the `spec_id` disclosure.** The `Ready` and
   `run_id: None` paths append "spec_id filter is not applied by this query";
   the four `Unavailable` arms do not. Not a regression (those paths produced
   no note at all before), but an operator reading an `Unavailable` note gets
   no signal that their `spec_id` was ignored.

6. **Panic arm logs at `warn!`** despite its own comment saying it warrants a
   louder level than the ordinary timeout arm. Cosmetic mismatch.

## Test coverage

7. **Uncovered failure arms.** Committed tests do not exercise the timeout
   arm, panic arm, cancel arm, or the row-decode failure path in `query()`.
   The unreachable-server case was verified during development by a scratch
   test that was deleted, so that evidence is not reproducible from the repo.
   The `#[ignore]`d live test deletes the database out from under a connected
   store — a different error class than an unreachable server, though it
   routes through the same arm.

8. **Blackhole regression test costs ~10s** on every plain `cargo test` run
   because it reuses the production 10s `CONNECT_TIMEOUT`. A shorter
   test-only constant would reclaim that.

9. ~~**Loose assertion:** the blackhole test asserts `elapsed < 30s` against a
   10s timeout — a 3× margin that would not catch a regression to 25s.
   Tighten to ~15s.~~ **Done** — the bound is now derived as
   `CONNECT_TIMEOUT + 5s` rather than a literal, so it tracks the constant it
   exists to defend. Measured elapsed is 10.00s, leaving 5s of headroom.

10. **e2e nits:** `result.database` / `result.backend` assertions are
    tautological (echoed from the config the test itself built), and
    `result.query` is unasserted. The assertions carrying the actual proof
    (hit count, labels, confidences, sort order, decoy exclusion) are sound
    and were mutation-verified.

11. ~~**`tests/typedb_connectivity.rs`** — import not rustfmt-sorted; the test
    leaves its `harkonnen_connectivity_check` database behind.~~ **Done** —
    the import was sorted by the `cargo fmt` pass in `f0c8be3`; the test now
    drops any stale database before creating its own and deletes it at the
    end, asserting the removal. The sibling test in
    `tests/typedb_schema_deploy.rs` had the same flaw and was given the same
    treatment, though it was never named by this item.

## Configuration / deployment

12. ~~**`docker-compose.calvin.yml` pins `typedb/typedb:latest`** while the
    driver is pinned to `3.12` and everything was validated against 3.12.1.
    A `latest` bump reproduces exactly the wire-protocol mismatch that cost
    Task 1 three rounds. Pin to `typedb/typedb:3.12.1`.~~ **Done** — pinned
    to `typedb/typedb:3.12.1` in `docker-compose.calvin.yml` and in
    `scripts/bootstrap-calvin-archive-typedb.sh`, which carried the same
    unpinned `latest` and was missed when this item was written.

13. **`harkonnen.toml` now sets `enabled = true` repo-wide**, so CI
    (`.github/workflows/benchmarks.yml`, which runs without
    `HARKONNEN_SETUP`) attempts a TypeDB connect and logs a warning each run.
    Benign (~55ms connection-refused), but consider leaving the repo default
    `false` and enabling only in `setups/home-linux.toml`.

14. **Hardcoded TypeDB credentials.** `Credentials::new("admin", "password")`
    matches the existing `calvin/src/archive.rs` precedent and TypeDB's
    documented defaults; `TypeDbConfig` has no credentials fields. Fine while
    TypeDB is bound to loopback — **unacceptable if port 1729 is ever exposed
    beyond localhost.** Write that assumption down alongside any change.

15. **`SCHEMA_TQL` drift risk.** It is `include_str!`'d from a fixed
    compile-time path while `CausalGraphConfig.schema_path` is never read for
    the deploy. Zero impact today (both point at the same file), but
    `schema_path` is exposed in the status response, so the two can diverge.

## Roadmap

16. **ROADMAP.md Phase 6 is not annotated as shipped.** The repo convention is
    to mark delivered items inline. The read/query adapter is done; write-back
    (promoted lessons and causal links → TypeDB) remains explicitly open, as
    do the GAIA Level 3 live harness and the AgentBench adapters.

## Tool gateway

Recorded 2026-08-02. Not part of the Phase 6 work — surfaced while running the
factory end-to-end against a real browser-JS target on this branch. Kept here
because this is where this branch's deferred items live.

17. **`node` is missing from the auto-approved host-command list.**
    `assess_host_command_surface` (`src/orchestrator.rs:29093`, list at `:29097`)
    treats
    `cargo | go | python | python3 | pytest | make | npm | pnpm | yarn` as
    low-risk local build/test tooling with `approval_required: false`, and
    everything else as `risk: high` / `approval_required: true` on the grounds
    that it "executes an external process outside the model context".

    `node` is absent, so a spec whose `test_commands` use `node --check` or
    `node script.js` opens an approval blocker, while the same project's
    `npm run test` — which merely shells out to `node` — auto-approves. The
    distinction is not about risk; `npm` is the strictly larger surface, since
    it can also install and execute dependency code. It reads like an omission
    from the list rather than a deliberate exclusion.

    This bites any project without a `package.json`. A plain browser-JS or
    static project has no npm entry point, so `node` is its *only* way to run
    a syntax check or a test, and every such command lands in the gateway.

    Fix is a one-word addition to the array, but confirm the intent first: if
    the list is meant to be "tools that cannot execute arbitrary user code",
    then `npm`, `make`, and `python3` do not belong on it either, and the
    right change is to rethink the classification rather than extend the list.

## Setup reporting

18. **`setup check` reports `[ok]` for an API key that is set but empty.**
    `print_provider_status` (`src/cli.rs:1857`) decides with
    `std::env::var(&c.api_key_env).is_ok()`. `env::var` returns `Ok("")` for a
    variable that exists with an empty value, so a `.env` containing
    `GEMINI_API_KEY=` reports the provider as healthy. The failure then
    surfaces much later and much further away, as a provider-side
    `403 PERMISSION_DENIED — Method doesn't allow unregistered callers`, at the
    moment an agent first tries to use it.

    Cost of the gap is real: a run can complete planning, auto-approve its
    transaction boundary, and die in the edit lane, all while `setup check`
    insists the provider is fine. The check should treat an empty or
    whitespace-only value as missing.

    The same function has an inverse false negative. A local OpenAI-compatible
    provider (LM Studio) legitimately sets `api_key_env = ""` because the
    endpoint needs no auth — `optional_api_key` (`src/llm.rs:297`) handles this
    correctly and there is a test for it — but `setup check` still prints
    `[MISSING]`, because an empty *env var name* also fails `env::var`. So the
    keyless-local case is reported as broken while the empty-key case is
    reported as fine, which is exactly backwards. Both directions come from the
    same line and should be fixed together.

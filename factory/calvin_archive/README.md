# Soul Store Bootstrap

This directory is the first bootstrap of Soul Store as a first-class subsystem.

It is intentionally narrow:

- the TypeDB schema is a minimal continuity schema focused on Coobie's identity kernel
- the first seed dataset captures what does **not** change about Coobie
- the human-readable JSON projection mirrors the canonical bootstrap in a form that is easy to inspect in Git

## Files

- `typedb/schema.tql`
  Minimal TypeDB 3.x schema for Soul Store bootstrap.
- `typedb/coobie_kernel_seed.tql`
  Seed data for Coobie's invariant soul kernel.
- `projections/coobie_identity_kernel.json`
  Inspectable projection of the same seed model for humans and tooling.
- `coobie_soul_guide.md`
  Plain-English explanation of what Coobie preserves and what changes are allowed.

## Generate / Refresh

Run:

```sh
cargo run -- soul bootstrap
```

That command refreshes the schema, seed, and JSON projection from the typed Rust source in `src/soul_store/`.

## Boot A Local TypeDB

A Docker-based bootstrap scaffold is available at:

```sh
./scripts/bootstrap-soul-store-typedb.sh
```

That script writes a local TypeDB compose stack and helper wrappers in the same style as the Coobie memory stack bootstrap.

## Why This Starts With Coobie

Coobie is the right first persisted self because she already spans:

- memory retrieval
- causal reasoning
- cross-run continuity
- operator-reviewed consolidation

If the continuity model cannot make Coobie legible, it is too weak for the rest of the pack.

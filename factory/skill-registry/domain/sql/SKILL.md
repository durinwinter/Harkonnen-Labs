---
name: sql
description: "SQL database patterns: schema migrations, query safety, index design, and transaction hygiene for this repo."
user-invocable: false
allowed-tools: []
---

# SQL Domain Guide

This repo uses SQL databases. Apply these patterns.

## Schema Migrations

- Every schema change is a migration file: never mutate the schema directly in production.
- Migrations must be reversible where possible — write a `down` migration alongside `up`.
- Test migrations on a snapshot before applying to production.
- Never drop a column in the same migration that removes references to it — split into two deploys.

## Query Safety

- Parameterize all user-supplied values. No string concatenation in queries.
- `SELECT *` is banned in application code — list columns explicitly to avoid silent breakage on schema changes.
- `DELETE` and `UPDATE` without a `WHERE` clause must never appear in application code.
- Wrap large data mutations in explicit transactions with rollback on error.

## Index Design

- Add indexes for every foreign key if the ORM/framework doesn't do it automatically.
- Composite indexes: most selective column first.
- Partial indexes for filtered queries (e.g., `WHERE deleted_at IS NULL`).
- Do not add indexes speculatively — measure first with `EXPLAIN ANALYZE`.

## Transaction Hygiene

- Keep transactions short: no external HTTP calls inside an open transaction.
- Use `SERIALIZABLE` isolation only when you can demonstrate the need — it increases contention.
- On deadlock: log the query, retry with backoff, escalate if persistent.

## Migration Tools

- Diesel (Rust): `diesel migration run`, `diesel migration redo`
- Alembic (Python): `alembic upgrade head`, `alembic downgrade -1`
- Flyway: `flyway migrate`, `flyway validate`
- Never mix migration tools on the same schema.

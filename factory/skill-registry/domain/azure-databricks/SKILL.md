---
name: azure-databricks
description: "Databricks on Azure: cluster lifecycle, Delta Lake operations, MLflow tracking, and Unity Catalog patterns for this repo."
user-invocable: false
allowed-tools:
  - Bash(databricks *)
  - WebFetch(docs.databricks.com)
---

# Azure Databricks Domain Guide

This repo uses Azure Databricks. Apply these patterns.

## Cluster Lifecycle

- Never create interactive clusters when job clusters suffice — job clusters are cheaper and auto-terminate.
- Pin cluster runtime versions explicitly; do not rely on `latest`.
- Check cluster state before submitting jobs: `databricks clusters get --cluster-id <id>`.
- Auto-termination: always set `autotermination_minutes` on interactive clusters.

## Delta Lake

- Always read with `spark.read.format("delta")` — not `parquet` — for managed Delta tables.
- Schema evolution: use `.option("mergeSchema", "true")` only when schema drift is intentional.
- Vacuuming: `VACUUM tablename RETAIN 168 HOURS` — never vacuum below the 7-day default without explicit approval.
- Z-ORDER by query columns, not partition columns.

## MLflow

- Log every experiment run: `mlflow.set_experiment("name")` before `mlflow.start_run()`.
- Log parameters, metrics, and artifacts explicitly — autolog misses edge cases.
- Registered models go in Unity Catalog: `models:/CatalogName.SchemaName.ModelName/version`.

## Unity Catalog

- Use three-part names: `catalog.schema.table`.
- Never use `hive_metastore` for new tables — that's the legacy path.
- Check grants before assuming read/write access: `SHOW GRANTS ON TABLE catalog.schema.table`.

## Safety

- Treat production clusters as read-first. Never write to production Delta tables without explicit approval.
- Databricks secrets must be stored in secret scopes, not in notebook cells or job configs.
- Use service principals for automation — not personal access tokens committed to source.

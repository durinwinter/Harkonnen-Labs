---
name: winccoa
description: "WinCC OA SCADA: CTRL scripts, panels, datapoint operations, manager lifecycle, and live-system safety for this repo."
user-invocable: false
allowed-tools: []
---

# WinCC OA Domain Guide

This repo involves WinCC OA (PVSS). Apply these patterns.

## Core Safety Rule

Treat CTRL scripts, panels, datapoints, managers, alerting, and runtime actions as operationally sensitive.
Any action that could affect a live plant, station, or operator workflow requires explicit human approval.

## CTRL Scripts

- Prefer read-first investigation before modifying any CTRL script.
- Test against offline exports, simulators, and staged project copies — not live runtime.
- CTRL is case-sensitive for variable names; be precise.
- Always null-check datapoint references before read/write: `dpExists()` before `dpGet()`/`dpSet()`.
- `dpSet()` on a live production datapoint is a write to a running plant — flag this to Keeper before doing it.

## Panels

- Be precise about panel paths (e.g., `panels/vision/faceplate/EXAMPLE.pnl`).
- Panel changes affect the operator's live HMI view — staging and review are mandatory before deployment.
- Avoid modifying shared library panels (`para/`, `vision/`) without understanding downstream impact.

## Datapoint Model

- Understand the datapoint schema before writing: check type, config elements, and archiving settings.
- `dpCreate()` and `dpDelete()` are irreversible on live systems without a backup.
- Use `dpQuery()` for inspection — it is read-only and safe.

## Manager Lifecycle

- Managers (`WCCOAui`, `WCCOActrl`, `WCCOAdp`) are long-lived processes — restarting them has operational impact.
- Check manager status before assuming it is safe to restart: use the GEDI project explorer or `pvsstatus`.
- Prefer offline WinCC OA topology sketches and mocked datapoints over runtime mutation during development.

## Deployment

- Export project backups before any structural change: `pvss_log`, panel exports, DP backups.
- Staged rollout: apply to a test project, verify with operators, then apply to production.
- Document every structural datapoint or panel change in the project changelog.

## Keeper Escalation

- Live SCADA, OT, operator workflow, and plant-facing changes are high risk by default.
- If uncertain whether an action is safe on a live system, escalate to Keeper before proceeding.

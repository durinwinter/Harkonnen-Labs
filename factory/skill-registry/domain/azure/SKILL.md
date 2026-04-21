---
name: azure
description: "Azure cloud resources: ARM/Bicep templates, Azure CLI patterns, RBAC, service principals, and subscription safety for this repo."
user-invocable: false
allowed-tools:
  - Bash(az *)
  - WebFetch(learn.microsoft.com)
---

# Azure Domain Guide

This repo uses Azure. Apply these patterns.

## Azure CLI

- Always confirm the active subscription before making changes: `az account show`.
- Use `--output json` for scripting; `--output table` for human inspection.
- Prefer `az resource list --resource-group <rg>` before modifying unknown resource groups.
- Set default subscription and group once per session: `az configure --defaults group=<rg> location=eastus`.

## Bicep / ARM Templates

- Parameterize environment-specific values (SKUs, names, locations) — never hardcode.
- Use `@secure()` decorator for passwords and keys — they will not appear in deployment logs.
- Validate before deploying: `az deployment group validate --resource-group <rg> --template-file main.bicep`.
- Preview changes: `az deployment group what-if --resource-group <rg> --template-file main.bicep`.

## RBAC

- Apply least-privilege: prefer built-in roles (`Reader`, `Contributor`) over custom roles.
- Never assign `Owner` to a service principal without explicit human approval.
- Check existing assignments before granting: `az role assignment list --assignee <principal-id>`.

## Service Principals

- Rotate secrets on a schedule — never reuse expired credentials.
- Store credentials in Azure Key Vault or CI secret stores, not in environment files committed to git.
- Use managed identities where possible — avoid password-based service principals for new workloads.

## Safety

- Treat production resource groups as read-first. Any destructive operation (`az resource delete`, `az group delete`) requires explicit approval.
- Tag all new resources with `env`, `project`, and `owner` tags.
- Deleting a resource group is irreversible — always confirm the group name before running `az group delete`.

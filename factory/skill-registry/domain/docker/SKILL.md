---
name: docker
description: "Docker and Compose: image builds, container lifecycle, networking, volume management, and safety conventions for this repo."
user-invocable: false
allowed-tools:
  - Bash(docker *)
  - Bash(docker-compose *)
  - Bash(podman *)
---

# Docker Domain Guide

This repo uses Docker. Apply these patterns.

## Dockerfile Conventions

- Pin base image tags: `FROM rust:1.78-slim-bookworm`, not `FROM rust:latest`.
- Use multi-stage builds to keep final images small.
- `COPY` specific files/directories — never `COPY . .` in the final stage.
- Run as non-root: add `USER nonroot` after installing dependencies.
- Combine `RUN` commands with `&&` to reduce layers; clean package caches in the same layer.

## Image Builds

- Build with `--no-cache` in CI to catch cache-corruption bugs.
- Tag with both `sha` and semantic version: never push `latest` as the only tag to production.
- Scan images for vulnerabilities before pushing: `docker scout cves image:tag`.

## Container Lifecycle

- Containers are ephemeral — do not store state inside them; use volumes or object storage.
- `docker logs <container> --tail 50 --follow` for live debugging.
- `docker exec -it <container> sh` for interactive inspection — avoid in production.
- Always set resource limits in production: `--memory`, `--cpus`.

## Compose

- Use named volumes, not bind mounts, for data that must persist across container restarts.
- Declare `depends_on` with `condition: service_healthy` when ordering matters.
- Override production credentials via `.env` or environment injection — never commit secrets to `compose.yml`.

## Networking

- Use named networks; avoid the default bridge network for multi-container projects.
- Expose ports minimally: prefer internal service names for inter-container communication.
- Do not bind to `0.0.0.0` in production without a firewall rule in place.

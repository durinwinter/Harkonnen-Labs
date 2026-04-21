---
name: node
description: "Node.js and TypeScript conventions for this repo: package management, build tooling, type safety, and testing patterns."
user-invocable: false
allowed-tools:
  - Bash(npm *)
  - Bash(pnpm *)
  - Bash(yarn *)
  - Bash(npx *)
  - Bash(node *)
---

# Node.js / TypeScript Project Guide

This repo uses Node.js. Apply these conventions.

## Package Management

- Use the package manager declared in `package.json` (`packageManager` field or lock file presence).
- Never mix `npm`, `yarn`, and `pnpm` in the same repo — pick one and stick with it.
- Pin exact versions for production dependencies in critical services; use `^` ranges for dev tooling.
- `npm ci` (not `npm install`) in CI — it respects the lockfile exactly.

## TypeScript

- `strict: true` in `tsconfig.json` — no implicit `any`.
- Prefer `interface` for object shapes that will be extended; `type` for unions and aliases.
- Avoid `as` casts except at serialization boundaries — they silence the type checker.
- `unknown` instead of `any` for values from external sources; narrow before use.

## Build Tooling

- Vite: `vite build` for production; `vite dev` for development.
- esbuild/tsup: for library builds; configure `entry`, `format`, and `dts` explicitly.
- Never import from `dist/` in source files — fix the path alias instead.

## Module Conventions

- Use ES modules (`"type": "module"` in `package.json`) for new projects.
- Named exports are preferred over default exports for better refactoring support.
- Barrel files (`index.ts`) for public APIs; avoid deep re-exporting.

## Testing

- Vitest for unit tests (matches Vite config); Jest for legacy projects.
- Test files alongside source: `foo.ts` → `foo.test.ts`.
- Use `vi.mock()` / `jest.mock()` at module level — not inside test bodies.
- Integration tests must not share global state; reset before each test.

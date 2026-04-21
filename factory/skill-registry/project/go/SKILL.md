---
name: go
description: "Go module conventions for this repo: idiomatic error handling, goroutines, testing patterns, and toolchain hygiene."
user-invocable: false
allowed-tools:
  - Bash(go *)
---

# Go Project Guide

This repo uses Go. Apply these conventions.

## Modules

- `go.mod` declares the module path and minimum Go version — do not edit manually without `go mod` commands.
- `go mod tidy` after adding or removing imports to keep `go.sum` consistent.
- Use `go mod vendor` in projects that require hermetic builds; add `vendor/` to git.
- Pin indirect dependencies only when a transitive vulnerability requires it.

## Error Handling

- Return errors as the last value; check them at every call site.
- Never ignore an error with `_` unless the function's signature makes it semantically safe.
- Wrap errors with context: `fmt.Errorf("reading config: %w", err)` — the `%w` verb enables `errors.Is`/`errors.As`.
- Define sentinel errors as `var ErrSomething = errors.New("...")` in the package that owns them.

## Goroutines

- Every goroutine must have a clear exit condition — no goroutine leaks.
- Use `context.Context` for cancellation propagation; pass it as the first argument.
- Prefer `sync.WaitGroup` for fan-out + join; prefer channels for producer/consumer patterns.
- Race detector in CI: `go test -race ./...`.

## Testing

- Test files end in `_test.go` and live alongside the code they test.
- Table-driven tests: `[]struct{ name, input, want }` with `t.Run(tc.name, ...)`.
- Use `t.Helper()` in assertion helpers so failure lines point to the call site.
- Benchmarks use `func BenchmarkXxx(b *testing.B)` — run with `go test -bench=.`.

## Formatting

- `gofmt` and `goimports` are non-negotiable — run in CI; reject unformatted code.
- `go vet ./...` catches common mistakes; treat warnings as errors.
- `golangci-lint` for broader static analysis.

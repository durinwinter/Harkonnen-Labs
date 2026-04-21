---
name: python
description: "Python conventions for this repo: virtual environments, type hints, pyproject.toml, and pytest patterns."
user-invocable: false
allowed-tools:
  - Bash(python *)
  - Bash(python3 *)
  - Bash(pip *)
  - Bash(uv *)
  - Bash(pytest *)
---

# Python Project Guide

This repo uses Python. Apply these conventions.

## Environment

- Always work inside a virtual environment — never install to the global interpreter.
- Prefer `uv` for speed (`uv venv`, `uv pip install`); fall back to `python -m venv` + `pip`.
- Activate before running: `source .venv/bin/activate` (Linux/macOS) or `.venv\Scripts\activate` (Windows).
- `pyproject.toml` is the single source of truth for dependencies and tool config.

## Type Hints

- All function signatures must have type hints.
- Use `from __future__ import annotations` for forward references.
- Run `mypy` or `pyright` in CI — treat type errors as build failures.
- Use `TypedDict` for structured dict shapes; `dataclass` for mutable value objects.

## Code Style

- Use `ruff` for linting and formatting: `ruff check .` and `ruff format .`.
- Maximum line length: 100 characters (configured in `pyproject.toml`).
- `f-strings` for interpolation — not `%` or `.format()`.
- No bare `except:` — always name the exception: `except ValueError as e:`.

## Testing

- Tests in `tests/` directory; test files prefixed with `test_`.
- `pytest -q` for compact output; `pytest -x` to stop on first failure.
- Use `pytest.fixture` for setup/teardown — not `setUp`/`tearDown` (unittest style).
- Mock with `unittest.mock.patch` or `pytest-mock`'s `mocker` fixture.
- Parametrize with `@pytest.mark.parametrize` rather than writing loop-based test cases.

## Packaging

- Declare all dependencies in `[project.dependencies]` in `pyproject.toml`.
- Dev dependencies in `[project.optional-dependencies] dev = [...]`.
- Do not use `setup.py` for new projects — `pyproject.toml` only.

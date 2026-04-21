#!/usr/bin/env python3
"""
DevBench -> Harkonnen bridge scaffold.

This script turns one DevBench repository task into:
1. A validated adapter summary artifact.
2. A draft Harkonnen YAML spec.
3. An optional launched Harkonnen run command.

It is intentionally conservative: it reads the official DevBench repo layout and
`repo_config.json`, validates the referenced files, derives a first-pass spec,
and leaves the final execution command configurable via CLI or environment.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SUPPORTED_TASKS = {
    "SoftwareDesign",
    "EnvironmentSetup",
    "Implementation",
    "AcceptanceTesting",
    "UnitTesting",
}

DEFAULT_OUTPUT_ROOT = Path("factory/artifacts/benchmarks/devbench")


@dataclass
class RepoContext:
    task: str
    repo_path: Path
    project_name: str
    config_path: Path
    config: dict[str, Any]
    output_dir: Path
    spec_path: Path
    generated_at: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a Harkonnen draft spec from a DevBench repository task."
    )
    parser.add_argument(
        "--task",
        choices=sorted(SUPPORTED_TASKS),
        default=os.environ.get("DEVBENCH_TASK", "Implementation"),
        help="DevBench task to adapt.",
    )
    parser.add_argument(
        "--repo",
        required=True,
        help="Path to a DevBench repo directory, absolute or relative to DEVBENCH_ROOT.",
    )
    parser.add_argument(
        "--devbench-root",
        default=os.environ.get("DEVBENCH_ROOT", ""),
        help="Optional DevBench checkout root used to resolve --repo.",
    )
    parser.add_argument(
        "--project-name",
        default="",
        help="Optional override for the project name. Defaults to the repo directory name.",
    )
    parser.add_argument(
        "--output-dir",
        default=os.environ.get("DEVBENCH_OUTPUT", ""),
        help="Output directory for summary artifacts and generated spec.",
    )
    parser.add_argument(
        "--spec-path",
        default="",
        help="Optional explicit output path for the generated Harkonnen YAML spec.",
    )
    parser.add_argument(
        "--setup",
        default=os.environ.get("HARKONNEN_SETUP", ""),
        help="Optional Harkonnen setup name to include in summary text.",
    )
    parser.add_argument(
        "--run-command",
        default=os.environ.get("DEVBENCH_HARKONNEN_RUN_COMMAND", ""),
        help=(
            "Optional shell command to launch Harkonnen after generating the spec. "
            "Supports placeholders: {spec_path}, {repo_path}, {task}, "
            "{project_name}, {output_dir}, {setup}."
        ),
    )
    parser.add_argument(
        "--launch",
        action="store_true",
        default=os.environ.get("DEVBENCH_LAUNCH", "").strip().lower()
        in {"1", "true", "yes", "on"},
        help="Execute --run-command after generating the spec.",
    )
    return parser.parse_args()


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def resolve_repo_path(repo_value: str, devbench_root: str) -> Path:
    raw = Path(repo_value)
    if raw.is_absolute():
        return raw.resolve()

    if devbench_root:
        return (Path(devbench_root) / raw).resolve()

    return raw.resolve()


def load_repo_context(args: argparse.Namespace) -> RepoContext:
    repo_path = resolve_repo_path(args.repo, args.devbench_root)
    if not repo_path.exists():
        raise SystemExit(f"DevBench repo path does not exist: {repo_path}")
    if not repo_path.is_dir():
        raise SystemExit(f"DevBench repo path is not a directory: {repo_path}")

    config_path = repo_path / "repo_config.json"
    if not config_path.exists():
        raise SystemExit(f"Missing repo_config.json in DevBench repo: {repo_path}")

    with config_path.open("r", encoding="utf-8") as handle:
        config = json.load(handle)

    project_name = args.project_name or repo_path.name
    output_dir = (
        Path(args.output_dir)
        if args.output_dir
        else DEFAULT_OUTPUT_ROOT / "adapter" / sanitize_identifier(project_name) / args.task.lower()
    )
    spec_path = (
        Path(args.spec_path)
        if args.spec_path
        else output_dir / "harkonnen_spec.yaml"
    )

    return RepoContext(
        task=args.task,
        repo_path=repo_path,
        project_name=project_name,
        config_path=config_path,
        config=config,
        output_dir=output_dir,
        spec_path=spec_path,
        generated_at=utc_now_iso(),
    )


def sanitize_identifier(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", value.strip().lower()).strip("-")
    return slug or "devbench-project"


def repo_relative(path: Path, base: Path) -> str:
    try:
        return path.relative_to(base).as_posix()
    except ValueError:
        return path.as_posix()


def resolve_config_path(repo_path: Path, raw_value: Any) -> Path | None:
    if not isinstance(raw_value, str) or not raw_value.strip():
        return None
    candidate = Path(raw_value)
    if candidate.is_absolute():
        return candidate.resolve()
    return (repo_path / candidate).resolve()


def resolve_optional_paths(repo_path: Path, raw_value: Any) -> list[Path]:
    if isinstance(raw_value, str) and raw_value.strip():
        return [resolve_config_path(repo_path, raw_value)]  # type: ignore[list-item]
    if isinstance(raw_value, list):
        resolved: list[Path] = []
        for item in raw_value:
            path = resolve_config_path(repo_path, item)
            if path is not None:
                resolved.append(path)
        return resolved
    return []


def validate_repo(ctx: RepoContext) -> tuple[list[str], list[str], dict[str, str]]:
    warnings: list[str] = []
    errors: list[str] = []
    resolved: dict[str, str] = {}

    required_config_keys = {
        "SoftwareDesign": ["PRD"],
        "EnvironmentSetup": ["PRD", "UML_class", "UML_sequence", "architecture_design"],
        "Implementation": [
            "PRD",
            "UML_class",
            "UML_sequence",
            "architecture_design",
            "setup_shell_script",
            "required_files",
        ],
        "AcceptanceTesting": [
            "PRD",
            "UML_class",
            "UML_sequence",
            "architecture_design",
            "acceptance_tests",
            "acceptance_test_script",
        ],
        "UnitTesting": [
            "PRD",
            "UML_class",
            "UML_sequence",
            "architecture_design",
            "unit_tests",
            "unit_test_script",
        ],
    }

    for key in required_config_keys[ctx.task]:
        value = ctx.config.get(key)
        if value in (None, "", [], {}):
            errors.append(f"repo_config.json is missing required field `{key}` for task {ctx.task}.")

    common_paths = {
        "PRD": ctx.config.get("PRD"),
        "UML_class": ctx.config.get("UML_class"),
        "UML_sequence": ctx.config.get("UML_sequence"),
        "dependencies": ctx.config.get("dependencies"),
        "architecture_design": ctx.config.get("architecture_design"),
        "usage_examples": ctx.config.get("usage_examples"),
        "unit_tests": ctx.config.get("unit_tests"),
        "acceptance_tests": ctx.config.get("acceptance_tests"),
        "setup_shell_script": ctx.config.get("setup_shell_script"),
    }

    for label, raw_value in common_paths.items():
        path = resolve_config_path(ctx.repo_path, raw_value)
        if path is None:
            continue
        resolved[label] = repo_relative(path, ctx.repo_path)
        if not path.exists():
            warnings.append(f"Referenced path `{label}` does not exist: {path}")

    for raw_path in resolve_optional_paths(ctx.repo_path, ctx.config.get("required_files")):
        if not raw_path.exists():
            warnings.append(f"Required source file listed in repo_config is missing: {raw_path}")

    if ctx.task == "Implementation":
        if not ctx.config.get("acceptance_test_script") and not ctx.config.get("unit_test_script"):
            warnings.append(
                "Implementation task has neither `acceptance_test_script` nor `unit_test_script`; "
                "generated spec will have limited visible validation."
            )

    return warnings, errors, resolved


def task_scope(task: str) -> list[str]:
    mapping = {
        "SoftwareDesign": [
            "read the DevBench PRD and existing repository context",
            "produce software-design artifacts aligned with the benchmark task",
            "keep the work bounded to design artifacts rather than implementation",
        ],
        "EnvironmentSetup": [
            "derive environment setup steps from the DevBench repository docs",
            "prepare dependency or build configuration artifacts needed by the repo",
            "validate example usage or setup execution where the benchmark config allows it",
        ],
        "Implementation": [
            "implement the repository files required by the DevBench task",
            "respect the repository architecture design and code-file order when available",
            "run the visible acceptance and unit checks declared by the benchmark config",
        ],
        "AcceptanceTesting": [
            "author acceptance tests against the repository requirements and architecture",
            "keep implementation source as the reference target rather than rewriting product code",
            "run the acceptance test command defined by the benchmark config",
        ],
        "UnitTesting": [
            "author unit tests aligned with the repository architecture and code layout",
            "prefer targeted test generation over product implementation changes",
            "run the unit test command defined by the benchmark config",
        ],
    }
    return mapping[task]


def task_constraints(task: str) -> list[str]:
    base = [
        "preserve the repository's existing benchmark-owned docs and evaluation assets",
        "do not modify Harkonnen factory memory or hidden scenario stores",
        "keep changes bounded to files relevant to the selected DevBench task",
    ]
    if task in {"AcceptanceTesting", "UnitTesting"}:
        base.append("treat the benchmark implementation as the reference target for generated tests")
    if task == "SoftwareDesign":
        base.append("do not skip required design artifacts by jumping directly to implementation code")
    return base


def derive_inputs(ctx: RepoContext, resolved_paths: dict[str, str]) -> list[str]:
    items = [f"devbench repo: {ctx.repo_path}"]
    for label in [
        "PRD",
        "UML_class",
        "UML_sequence",
        "architecture_design",
        "dependencies",
        "usage_examples",
        "setup_shell_script",
    ]:
        if label in resolved_paths:
            items.append(f"{label}: {resolved_paths[label]}")
    items.append(f"repo_config: {repo_relative(ctx.config_path, ctx.repo_path)}")
    return items


def derive_outputs(ctx: RepoContext) -> list[str]:
    outputs = [
        repo_relative(ctx.spec_path, Path.cwd()),
        repo_relative(ctx.output_dir / "devbench_adapter_summary.json", Path.cwd()),
        repo_relative(ctx.output_dir / "devbench_adapter_summary.md", Path.cwd()),
    ]
    if ctx.task == "SoftwareDesign":
        outputs.extend(
            [
                f"{ctx.project_name}/docs/UML_class.md or equivalent design artifact",
                f"{ctx.project_name}/docs/UML_sequence.md or equivalent design artifact",
                f"{ctx.project_name}/docs/architecture_design.md or equivalent design artifact",
            ]
        )
    return outputs


def derive_test_commands(ctx: RepoContext) -> list[str]:
    commands: list[str] = []
    setup_script = ctx.config.get("setup_shell_script")
    setup_path = resolve_config_path(ctx.repo_path, setup_script)
    if setup_path is not None and ctx.task in {"EnvironmentSetup", "Implementation"}:
        commands.append(f"sh {shlex.quote(repo_relative(setup_path, ctx.repo_path))}")

    if ctx.task in {"Implementation", "UnitTesting"}:
        unit_cmd = ctx.config.get("unit_test_script")
        if isinstance(unit_cmd, str) and unit_cmd.strip():
            commands.append(unit_cmd.strip())

    if ctx.task in {"Implementation", "AcceptanceTesting"}:
        accept_cmd = ctx.config.get("acceptance_test_script")
        if isinstance(accept_cmd, str) and accept_cmd.strip():
            commands.append(accept_cmd.strip())

    return commands


def derive_acceptance_criteria(ctx: RepoContext) -> list[str]:
    criteria = {
        "SoftwareDesign": [
            "design artifacts remain faithful to the DevBench PRD",
            "class and sequence relationships are internally consistent",
            "architecture design is practical for the repository language and layout",
        ],
        "EnvironmentSetup": [
            "setup artifacts are sufficient for the repository's documented environment task",
            "setup or dependency command exits 0 when executed in the benchmark environment",
            "example usage path is runnable when the benchmark repo provides one",
        ],
        "Implementation": [
            "implementation changes stay consistent with the PRD and architecture design",
            "unit test script exits 0 when provided by the benchmark repo",
            "acceptance test script exits 0 when provided by the benchmark repo",
        ],
        "AcceptanceTesting": [
            "generated acceptance tests reflect repository-level requirements from the PRD",
            "acceptance test script exits 0 against the benchmark implementation",
            "generated tests exercise user-visible behavior rather than private implementation details",
        ],
        "UnitTesting": [
            "generated unit tests align with the repository architecture and file boundaries",
            "unit test script exits 0 against the benchmark implementation",
            "unit testing output includes the expected coverage artifact when the benchmark config requests it",
        ],
    }
    return criteria[ctx.task]


def derive_dependencies(ctx: RepoContext, resolved_paths: dict[str, str]) -> list[str]:
    deps = []
    language = str(ctx.config.get("language", "")).strip()
    if language:
        deps.append(f"language runtime for {language}")
    if "dependencies" in resolved_paths:
        deps.append(f"dependency file present at {resolved_paths['dependencies']}")
    if ctx.config.get("setup_shell_script"):
        deps.append("shell environment capable of running the benchmark setup script")
    if ctx.config.get("unit_test_script"):
        deps.append("test runner required by unit_test_script")
    if ctx.config.get("acceptance_test_script"):
        deps.append("test runner required by acceptance_test_script")
    return deps


def derive_spec(ctx: RepoContext, resolved_paths: dict[str, str]) -> dict[str, Any]:
    spec_id = f"devbench-{sanitize_identifier(ctx.project_name)}-{ctx.task.lower()}"
    language = str(ctx.config.get("language", "unknown")).strip().lower() or "unknown"
    purpose = (
        f"Execute the DevBench {ctx.task} task for {ctx.project_name} by translating the "
        "benchmark repo configuration into a bounded Harkonnen run."
    )
    spec: dict[str, Any] = {
        "id": spec_id,
        "title": f"DevBench {ctx.project_name} {ctx.task}",
        "purpose": purpose,
        "scope": task_scope(ctx.task),
        "constraints": task_constraints(ctx.task),
        "inputs": derive_inputs(ctx, resolved_paths),
        "outputs": derive_outputs(ctx),
        "acceptance_criteria": derive_acceptance_criteria(ctx),
        "forbidden_behaviors": [
            "editing benchmark-owned hidden evaluation assets to manufacture a pass",
            "ignoring repo_config.json execution boundaries",
            "rewriting unrelated repository files outside the selected task scope",
        ],
        "rollback_requirements": [
            "generated run artifacts can be removed without damaging the benchmark repo",
            "task-specific edits remain reviewable and reversible",
        ],
        "dependencies": derive_dependencies(ctx, resolved_paths),
        "performance_expectations": [
            "visible validation should be runnable through the repo_config-defined commands",
            "the adapter should preserve enough structure for later benchmark automation",
        ],
        "security_expectations": [
            "do not leak local credentials or API keys into generated artifacts",
            "keep execution local to the benchmark workspace unless the user opts into networked services",
        ],
        "test_commands": derive_test_commands(ctx),
        "project_components": [
            {
                "name": sanitize_identifier(ctx.project_name),
                "role": "code_under_test" if ctx.task == "Implementation" else "benchmark_repo",
                "kind": f"devbench_{language}_repo",
                "path": str(ctx.repo_path),
                "owner": "devbench_dataset",
                "notes": [
                    f"Adapted from DevBench task {ctx.task}.",
                    "repo_config.json is the benchmark source of truth for visible execution.",
                ],
                "interfaces": [language, "repo_config.json"],
            }
        ],
        "worker_harness": {
            "adapter": "devbench_harkonnen_adapter",
            "profile": "devbench_scaffold",
            "allowed_components": [sanitize_identifier(ctx.project_name)],
            "denied_paths": ["factory/scenarios", "factory/memory"],
            "visible_success_conditions": [
                "generated artifacts match the selected DevBench task",
                "repo_config.json-driven visible commands complete successfully when configured",
            ],
            "return_artifacts": [
                "changed_files",
                "execution_log",
                "visible_validation_report",
                "rationale_summary",
            ],
            "continuity_file": "trail-state.json",
            "llm_edits": True,
        },
    }
    return spec


def yaml_scalar(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    text = str(value)
    if text == "" or re.search(r"[:#\-\[\]\{\}\n]|^\s|\s$", text):
        escaped = text.replace("\\", "\\\\").replace('"', '\\"')
        return f'"{escaped}"'
    return text


def dump_yaml(data: Any, indent: int = 0) -> str:
    prefix = " " * indent
    if isinstance(data, dict):
        lines: list[str] = []
        for key, value in data.items():
            if isinstance(value, (dict, list)):
                lines.append(f"{prefix}{key}:")
                lines.append(dump_yaml(value, indent + 2))
            else:
                lines.append(f"{prefix}{key}: {yaml_scalar(value)}")
        return "\n".join(lines)
    if isinstance(data, list):
        if not data:
            return f"{prefix}[]"
        lines = []
        for item in data:
            if isinstance(item, (dict, list)):
                rendered = dump_yaml(item, indent + 2)
                rendered_lines = rendered.splitlines()
                if rendered_lines:
                    lines.append(f"{prefix}- {rendered_lines[0].lstrip()}")
                    for extra in rendered_lines[1:]:
                        lines.append(extra)
                else:
                    lines.append(f"{prefix}-")
            else:
                lines.append(f"{prefix}- {yaml_scalar(item)}")
        return "\n".join(lines)
    return f"{prefix}{yaml_scalar(data)}"


def build_launch_command(args: argparse.Namespace, ctx: RepoContext) -> str:
    if not args.run_command:
        if not args.launch:
            return ""
        validate_cmd, run_cmd = recommended_commands(ctx, args)
        return f"{validate_cmd} && {run_cmd}"
    replacements = {
        "spec_path": str(ctx.spec_path),
        "repo_path": str(ctx.repo_path),
        "task": ctx.task,
        "project_name": ctx.project_name,
        "output_dir": str(ctx.output_dir),
        "setup": args.setup,
    }
    return args.run_command.format(**replacements)


def recommended_commands(ctx: RepoContext, args: argparse.Namespace) -> list[str]:
    validate_cmd = f"cargo run -- spec validate {shlex.quote(str(ctx.spec_path))}"
    run_cmd = (
        f"cargo run -- run start {shlex.quote(str(ctx.spec_path))} "
        f"--product-path {shlex.quote(str(ctx.repo_path))}"
    )
    if args.setup:
        run_cmd = f"HARKONNEN_SETUP={shlex.quote(args.setup)} {run_cmd}"
    return [validate_cmd, run_cmd]


def write_summary_markdown(summary: dict[str, Any]) -> str:
    lines = [
        "# DevBench Adapter Summary",
        "",
        f"- Generated: {summary['generated_at']}",
        f"- Task: {summary['task']}",
        f"- Project: {summary['project_name']}",
        f"- Repo: {summary['repo_path']}",
        f"- Generated spec: {summary['generated_spec_path']}",
        "",
        "## Validation",
        "",
        f"- Errors: {len(summary['errors'])}",
        f"- Warnings: {len(summary['warnings'])}",
    ]
    if summary["errors"]:
        lines.append("")
        lines.append("### Errors")
        for item in summary["errors"]:
            lines.append(f"- {item}")
    if summary["warnings"]:
        lines.append("")
        lines.append("### Warnings")
        for item in summary["warnings"]:
            lines.append(f"- {item}")

    lines.extend(
        [
            "",
            "## Visible Commands",
        ]
    )
    if summary["derived_test_commands"]:
        for command in summary["derived_test_commands"]:
            lines.append(f"- `{command}`")
    else:
        lines.append("- No visible commands were derived for this task.")

    lines.extend(
        [
            "",
            "## Recommended Harkonnen Commands",
        ]
    )
    for command in summary["recommended_commands"]:
        lines.append(f"- `{command}`")

    if summary.get("launch_command"):
        lines.extend(
            [
                "",
                "## Launch",
                "",
                f"- Launch command: `{summary['launch_command']}`",
                f"- Launch executed: `{summary['launch_executed']}`",
            ]
        )
        result = summary.get("launch_result")
        if result:
            lines.append(f"- Exit code: `{result['exit_code']}`")
            if result["exit_code"] == 0:
                lines.append("- Launch status: `passed`")
            else:
                lines.append("- Launch status: `failed`")

    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    ctx = load_repo_context(args)
    warnings, errors, resolved_paths = validate_repo(ctx)
    spec = derive_spec(ctx, resolved_paths)
    launch_command = build_launch_command(args, ctx)

    ctx.output_dir.mkdir(parents=True, exist_ok=True)
    ctx.spec_path.parent.mkdir(parents=True, exist_ok=True)
    ctx.spec_path.write_text(dump_yaml(spec) + "\n", encoding="utf-8")

    summary: dict[str, Any] = {
        "generated_at": ctx.generated_at,
        "task": ctx.task,
        "project_name": ctx.project_name,
        "repo_path": str(ctx.repo_path),
        "repo_config_path": str(ctx.config_path),
        "generated_spec_path": str(ctx.spec_path),
        "output_dir": str(ctx.output_dir),
        "language": ctx.config.get("language", ""),
        "errors": errors,
        "warnings": warnings,
        "resolved_paths": resolved_paths,
        "derived_test_commands": spec.get("test_commands", []),
        "recommended_commands": recommended_commands(ctx, args),
        "launch_command": launch_command,
        "launch_executed": bool(args.launch and launch_command),
    }

    launch_exit_code = 0
    if args.launch and launch_command:
        result = subprocess.run(
            launch_command,
            shell=True,
            text=True,
            capture_output=True,
            cwd=str(Path.cwd()),
        )
        summary["launch_result"] = {
            "exit_code": result.returncode,
            "stdout": result.stdout[-8000:],
            "stderr": result.stderr[-8000:],
        }
        launch_exit_code = result.returncode

    summary_json = ctx.output_dir / "devbench_adapter_summary.json"
    summary_md = ctx.output_dir / "devbench_adapter_summary.md"
    summary_json.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    summary_md.write_text(write_summary_markdown(summary), encoding="utf-8")

    print(f"DevBench repo: {ctx.repo_path}")
    print(f"Task: {ctx.task}")
    print(f"Generated spec: {ctx.spec_path}")
    print(f"Summary JSON: {summary_json}")
    print(f"Summary Markdown: {summary_md}")
    if errors:
        print(f"Validation errors: {len(errors)}")
    if warnings:
        print(f"Validation warnings: {len(warnings)}")
    if launch_command:
        print(f"Launch command: {launch_command}")
        if args.launch:
            launch_result = summary.get("launch_result", {})
            print(f"Launch exit code: {launch_result.get('exit_code')}")
    else:
        print("Recommended next commands:")
        for command in summary["recommended_commands"]:
            print(f"  {command}")

    if errors:
        return 2
    if args.launch and launch_command and launch_exit_code != 0:
        return launch_exit_code
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Capture stage 1 reviewer outputs for the llm-council workflow."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from textwrap import dedent
from typing import Callable

from cli_client import CLIModelError, ModelSpec, query_model

DEFAULT_MODELS = {
    "codex": "gpt-5.4",
    "claude": "opus",
    "gemini": "gemini-3.1-pro-preview",
}


def _unsafe_cli_enabled() -> bool:
    return os.getenv("LLM_COUNCIL_ALLOW_UNSAFE_CLI", "").strip().lower() in {"1", "true", "yes", "on"}


def _codex_review_flags() -> list[str]:
    if _unsafe_cli_enabled():
        return ["--dangerously-bypass-approvals-and-sandbox"]
    return []


def _codex_exec_flags() -> list[str]:
    if _unsafe_cli_enabled():
        return ["--yolo", "--skip-git-repo-check"]
    return []


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run stage 1 llm-council reviewers and write a stage1 artifact."
    )
    parser.add_argument("--repo", required=True, help="Repository root to review.")
    parser.add_argument("--output", required=True, help="Path to write the stage1 JSON artifact.")
    parser.add_argument("--user-query", required=True, help="Original review request.")
    parser.add_argument("--context-file", required=True, help="Path to the prepared review context file.")
    parser.add_argument("--base-rev", help="Exact base revision for diff-aware review tools.")
    parser.add_argument("--head-rev", help="Revision to check out in a temporary worktree before review.")
    parser.add_argument(
        "--reviewer",
        action="append",
        choices=("codex", "claude", "gemini"),
        help="Reviewer(s) to run. Repeatable. Defaults to all.",
    )
    parser.add_argument("--codex-model", default=DEFAULT_MODELS["codex"])
    parser.add_argument("--claude-model", default=DEFAULT_MODELS["claude"])
    parser.add_argument("--gemini-model", default=DEFAULT_MODELS["gemini"])
    parser.add_argument("--files-summary", help="Optional files summary for the stage1 artifact.")
    parser.add_argument("--language", help="Optional language for the stage1 artifact.")
    parser.add_argument("--timeout", type=float, default=600.0, help="Per-reviewer timeout in seconds.")
    parser.add_argument("--keep-worktree", action="store_true", help="Keep the temporary worktree on disk.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo = Path(args.repo).expanduser().resolve()
    context_file = Path(args.context_file).expanduser().resolve()
    output_path = Path(args.output).expanduser().resolve()

    ensure_git_repo(repo)
    if not context_file.is_file():
        raise CLIModelError(f"context file does not exist: {context_file}")

    reviewers = args.reviewer or ["codex", "claude", "gemini"]
    workdir, cleanup = prepare_workdir(repo, args.head_rev)
    try:
        stage1 = run_stage1(
            workdir=workdir,
            repo=repo,
            context_file=context_file,
            user_query=args.user_query,
            base_rev=args.base_rev,
            reviewers=reviewers,
            model_overrides={
                "codex": args.codex_model,
                "claude": args.claude_model,
                "gemini": args.gemini_model,
            },
            files_summary=args.files_summary or infer_files_summary(workdir, args.base_rev),
            language=args.language or infer_language(workdir, args.base_rev),
            timeout=args.timeout,
        )
        write_stage1(output_path, stage1)
    finally:
        if cleanup is not None and not args.keep_worktree:
            cleanup()

    return 0


def ensure_git_repo(repo: Path) -> None:
    try:
        completed = subprocess.run(
            ["git", "rev-parse", "--git-dir"],
            cwd=repo,
            text=True,
            capture_output=True,
            timeout=5,
            check=False,
        )
    except OSError as exc:
        raise CLIModelError(f"unable to inspect git repo at {repo}: {exc}") from exc

    if completed.returncode != 0:
        raise CLIModelError(f"not a git repository: {repo}")


def prepare_workdir(repo: Path, head_rev: str | None) -> tuple[Path, Callable[[], None] | None]:
    if not head_rev:
        return repo, None

    tempdir = Path(tempfile.mkdtemp(prefix="llm-council-worktree-"))
    command = ["git", "worktree", "add", "--detach", str(tempdir), head_rev]
    completed = subprocess.run(
        command,
        cwd=repo,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    if completed.returncode != 0:
        shutil.rmtree(tempdir, ignore_errors=True)
        raise CLIModelError(
            f"failed to create worktree for {head_rev}: "
            f"{(completed.stderr or completed.stdout).strip()}"
        )

    def cleanup() -> None:
        subprocess.run(
            ["git", "worktree", "remove", "--force", str(tempdir)],
            cwd=repo,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        shutil.rmtree(tempdir, ignore_errors=True)

    return tempdir, cleanup


def run_stage1(
    *,
    workdir: Path,
    repo: Path,
    context_file: Path,
    user_query: str,
    base_rev: str | None,
    reviewers: list[str],
    model_overrides: dict[str, str],
    files_summary: str,
    language: str,
    timeout: float,
) -> dict[str, object]:
    responses: list[dict[str, str]] = []
    failures: list[str] = []
    with ThreadPoolExecutor(max_workers=len(reviewers)) as executor:
        futures = {
            executor.submit(
                run_reviewer,
                reviewer,
                model_overrides[reviewer],
                workdir,
                repo,
                context_file,
                user_query,
                base_rev,
                timeout,
            ): reviewer
            for reviewer in reviewers
        }

        for future in as_completed(futures):
            reviewer = futures[future]
            try:
                result = future.result()
                if result is not None:
                    responses.append(result)
            except CLIModelError as exc:
                failures.append(f"{reviewer}: {exc}")

    responses.sort(key=lambda item: reviewers.index(item["source"].lower()))

    if not responses:
        raise CLIModelError(f"all stage1 reviewers failed: {'; '.join(failures)}")

    stage1: dict[str, object] = {
        "user_query": user_query,
        "files_summary": files_summary,
        "language": language,
        "responses": responses,
    }
    if failures:
        stage1["failures"] = failures
    return stage1


def run_reviewer(
    reviewer: str,
    model: str,
    workdir: Path,
    repo: Path,
    context_file: Path,
    user_query: str,
    base_rev: str | None,
    timeout: float,
) -> dict[str, str] | None:
    if reviewer == "codex":
        content = run_codex_reviewer(
            model=model,
            workdir=workdir,
            context_file=context_file,
            user_query=user_query,
            base_rev=base_rev,
            timeout=timeout,
        )
    elif reviewer == "claude":
        prompt = build_stage1_prompt(
            user_query,
            context_file,
            focus="architecture, maintainability, Go/proto idioms, and schema design",
        )
        content = query_model(ModelSpec("claude", model), prompt, timeout=timeout, cwd=str(workdir))["content"]
    elif reviewer == "gemini":
        prompt = build_stage1_prompt(
            user_query,
            context_file,
            focus="testing gaps, readability, API consistency, and best practices",
        )
        content = query_model(ModelSpec("gemini", model), prompt, timeout=timeout, cwd=str(workdir))["content"]
    else:
        raise CLIModelError(f"unsupported reviewer: {reviewer}")

    output_path = write_reviewer_output(reviewer, content)
    return {
        "source": reviewer.title(),
        "model": model,
        "path": str(output_path),
    }


def run_codex_reviewer(
    *,
    model: str,
    workdir: Path,
    context_file: Path,
    user_query: str,
    base_rev: str | None,
    timeout: float,
) -> str:
    binary = shutil.which("codex")
    if binary is None:
        raise CLIModelError("codex CLI is not installed or not on PATH")

    output_file = Path(tempfile.mkstemp(prefix="llm-council-codex-stage1-", suffix=".md")[1])
    try:
        if base_rev:
            command = [
                binary,
                "exec",
                "review",
                "--base",
                base_rev,
                "--model",
                model,
                "--config",
                'model_reasoning_effort="xhigh"',
                *_codex_review_flags(),
                "--ephemeral",
                "-o",
                str(output_file),
            ]
            prompt_text = None
        else:
            command = [
                binary,
                "exec",
                "--model",
                model,
                "--config",
                'model_reasoning_effort="xhigh"',
                *_codex_exec_flags(),
                "--ephemeral",
                "-o",
                str(output_file),
                "-",
            ]
            prompt_text = build_stage1_prompt(
                user_query,
                context_file,
                focus="security, performance, API contract correctness, and idiomatic patterns",
            )

        completed = subprocess.run(
            command,
            input=prompt_text,
            text=True,
            capture_output=True,
            cwd=workdir,
            timeout=timeout,
            check=False,
        )
        if completed.returncode != 0:
            stderr = (completed.stderr or completed.stdout).strip()
            raise CLIModelError(f"codex:{model} failed with exit {completed.returncode}: {stderr}")

        content = output_file.read_text(encoding="utf-8").strip()
        if not content:
            content = extract_codex_review_from_stderr(completed.stderr)
        if not content:
            raise CLIModelError(f"codex:{model} returned empty output")
        return content
    except subprocess.TimeoutExpired as exc:
        raise CLIModelError(f"codex:{model} timed out after {timeout}s") from exc
    finally:
        output_file.unlink(missing_ok=True)


CODEX_REVIEW_FALLBACK_RE = re.compile(
    r"(?ms)^codex\n(?P<content>.*?)(?:\nWarning: no last agent message;.*)?\Z"
)


def extract_codex_review_from_stderr(stderr: str | None) -> str:
    if not stderr:
        return ""

    matches = list(CODEX_REVIEW_FALLBACK_RE.finditer(stderr))
    if not matches:
        return ""

    return matches[-1].group("content").strip()


def build_stage1_prompt(user_query: str, context_file: Path, *, focus: str) -> str:
    return dedent(
        f"""
        {user_query}

        Read {context_file} before reviewing and focus on {focus}.
        Report concrete findings only. For each finding, include:
        - severity
        - file:line
        - why it matters

        If no issues are found, say so explicitly.
        """
    ).strip()


def write_reviewer_output(reviewer: str, content: str) -> Path:
    output_path = Path(tempfile.mkstemp(prefix=f"llm-council-{reviewer}-", suffix=".md")[1])
    output_path.write_text(content.rstrip() + "\n", encoding="utf-8")
    return output_path


def infer_files_summary(workdir: Path, base_rev: str | None) -> str:
    changed_files = git_lines(workdir, ["diff", "--name-only", base_rev] if base_rev else ["status", "--short"])
    files = [line.strip() for line in changed_files.splitlines() if line.strip()]
    if not files:
        return "captured-agent-outputs"
    if len(files) == 1:
        return files[0]
    preview = ", ".join(files[:3])
    if len(files) > 3:
        preview += f", +{len(files) - 3} more"
    return preview


def infer_language(workdir: Path, base_rev: str | None) -> str:
    changed_files = git_lines(workdir, ["diff", "--name-only", base_rev] if base_rev else ["status", "--short"])
    files = [line.strip() for line in changed_files.splitlines() if line.strip()]
    if files and all(file.endswith(".proto") for file in files):
        return "Protocol Buffers"
    if files and all(file.endswith(".go") for file in files):
        return "Go"
    return "Unknown"


def git_lines(workdir: Path, args: list[str]) -> str:
    command = ["git", *[arg for arg in args if arg]]
    completed = subprocess.run(
        command,
        cwd=workdir,
        text=True,
        capture_output=True,
        timeout=15,
        check=False,
    )
    if completed.returncode != 0:
        return ""
    return completed.stdout


def write_stage1(output_path: Path, payload: dict[str, object]) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CLIModelError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)

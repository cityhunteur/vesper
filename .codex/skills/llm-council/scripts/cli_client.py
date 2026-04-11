#!/usr/bin/env python3
"""CLI-based model execution helpers for llm-council scripts."""

from __future__ import annotations

import contextlib
import json
import os
import re
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from glob import glob
from pathlib import Path
from typing import Callable


class CLIModelError(RuntimeError):
    """Raised when a model CLI invocation fails."""


@dataclass(frozen=True)
class ModelSpec:
    """Executable CLI and model pairing."""

    cli: str
    model: str

    @property
    def label(self) -> str:
        return f"{self.cli}:{self.model}"


SUPPORTED_CLIS = {"codex", "claude", "gemini"}


def _unsafe_cli_enabled() -> bool:
    """Return whether unsafe automation flags are explicitly enabled."""
    return os.getenv("LLM_COUNCIL_ALLOW_UNSAFE_CLI", "").strip().lower() in {"1", "true", "yes", "on"}


def _codex_safety_flags() -> list[str]:
    if _unsafe_cli_enabled():
        return ["--yolo", "--skip-git-repo-check"]
    return []


def _claude_safety_flags() -> list[str]:
    if _unsafe_cli_enabled():
        return ["--dangerously-skip-permissions"]
    return []


def _gemini_safety_flags() -> list[str]:
    if _unsafe_cli_enabled():
        return ["--yolo"]
    return []


def parse_model_spec(value: str) -> ModelSpec:
    """Parse a `<cli>:<model>` string."""
    cli, separator, model = value.partition(":")
    cli_name = cli.strip().lower()
    model_name = model.strip()
    if not separator or not cli_name or not model_name:
        raise CLIModelError(
            f"invalid model spec '{value}'; expected format '<cli>:<model>'"
        )
    if cli_name not in SUPPORTED_CLIS:
        raise CLIModelError(
            f"unsupported cli '{cli_name}'; expected one of {sorted(SUPPORTED_CLIS)}"
        )
    return ModelSpec(cli=cli_name, model=model_name)


def query_model(
    spec: ModelSpec,
    prompt: str,
    *,
    timeout: float = 120.0,
    cwd: str | None = None,
) -> dict[str, str]:
    """Run a model CLI non-interactively and return normalized content."""
    prompt_text = prompt.strip()
    if not prompt_text:
        raise CLIModelError("prompt is required")

    binary = shutil.which(spec.cli)
    if binary is None:
        raise CLIModelError(f"{spec.cli} CLI is not installed or not on PATH")

    workdir = cwd or os.getcwd()

    if spec.cli == "codex":
        return _run_codex(binary, spec, prompt_text, timeout=timeout, cwd=workdir)
    if spec.cli == "claude":
        return _run_claude(binary, spec, prompt_text, timeout=timeout, cwd=workdir)
    if spec.cli == "gemini":
        return _run_gemini(binary, spec, prompt_text, timeout=timeout, cwd=workdir)

    raise CLIModelError(f"unsupported cli '{spec.cli}'")


def query_models_parallel(
    specs: list[ModelSpec],
    prompt: str,
    *,
    timeout: float = 120.0,
    cwd: str | None = None,
) -> list[dict[str, str | None]]:
    """Run multiple CLI models concurrently and preserve input order."""
    if not specs:
        raise CLIModelError("at least one model spec is required")

    results: list[dict[str, str | None] | None] = [None] * len(specs)
    with ThreadPoolExecutor(max_workers=len(specs)) as executor:
        future_to_index = {
            executor.submit(query_model, spec, prompt, timeout=timeout, cwd=cwd): index
            for index, spec in enumerate(specs)
        }
        for future in as_completed(future_to_index):
            index = future_to_index[future]
            spec = specs[index]
            try:
                response = future.result()
                results[index] = {
                    "cli": spec.cli,
                    "model": spec.model,
                    "content": response["content"],
                    "error": None,
                }
            except CLIModelError as exc:
                results[index] = {
                    "cli": spec.cli,
                    "model": spec.model,
                    "content": None,
                    "error": str(exc),
                }

    return [result for result in results if result is not None]


def _run_codex(
    binary: str,
    spec: ModelSpec,
    prompt: str,
    *,
    timeout: float,
    cwd: str,
) -> dict[str, str]:
    with tempfile.NamedTemporaryFile(prefix="llm-council-codex-", suffix=".md", delete=False) as handle:
        output_path = Path(handle.name)

    command = [
        binary,
        "exec",
        "--model",
        spec.model,
        "--config",
        'model_reasoning_effort="xhigh"',
        *_codex_safety_flags(),
        "-o",
        str(output_path),
        "-",
    ]
    try:
        completed = subprocess.run(
            command,
            input=prompt,
            text=True,
            capture_output=True,
            cwd=cwd,
            timeout=timeout,
            check=False,
        )
        if completed.returncode != 0:
            raise CLIModelError(
                f"{spec.label} failed with exit {completed.returncode}: "
                f"{tail_output(combine_process_output(completed))}"
            )
        content = output_path.read_text(encoding="utf-8").strip()
        if not content:
            raise CLIModelError(f"{spec.label} returned empty output")
        return {"content": content}
    except subprocess.TimeoutExpired as exc:
        raise CLIModelError(f"{spec.label} timed out after {timeout}s") from exc
    except OSError as exc:
        raise CLIModelError(f"{spec.label} failed to start: {exc}") from exc
    finally:
        with contextlib.suppress(OSError):
            output_path.unlink(missing_ok=True)


def _run_claude(
    binary: str,
    spec: ModelSpec,
    prompt: str,
    *,
    timeout: float,
    cwd: str,
) -> dict[str, str]:
    command = [
        binary,
        "-p",
        "--model",
        spec.model,
        *_claude_safety_flags(),
    ]
    return _run_stdio_cli(command, spec, prompt, timeout=timeout, cwd=cwd)


def _run_gemini(
    binary: str,
    spec: ModelSpec,
    prompt: str,
    *,
    timeout: float,
    cwd: str,
) -> dict[str, str]:
    command = [
        binary,
        "--model",
        spec.model,
        *_gemini_safety_flags(),
        "--output-format",
        "json",
        "-p",
        prompt,
    ]
    env = build_cli_env(spec.cli)
    return _run_stdio_cli(
        command,
        spec,
        prompt,
        timeout=timeout,
        cwd=cwd,
        env=env,
        parser=parse_gemini_response,
        use_stdin=False,
    )


def _run_stdio_cli(
    command: list[str],
    spec: ModelSpec,
    prompt: str,
    *,
    timeout: float,
    cwd: str,
    env: dict[str, str] | None = None,
    parser: Callable[[str], str] | None = None,
    use_stdin: bool = True,
) -> dict[str, str]:
    try:
        completed = subprocess.run(
            command,
            input=prompt if use_stdin else None,
            text=True,
            capture_output=True,
            cwd=cwd,
            env=env,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        raise CLIModelError(f"{spec.label} timed out after {timeout}s") from exc
    except OSError as exc:
        raise CLIModelError(f"{spec.label} failed to start: {exc}") from exc

    if completed.returncode != 0:
        raise CLIModelError(
            f"{spec.label} failed with exit {completed.returncode}: "
            f"{tail_output(combine_process_output(completed))}"
        )

    content = completed.stdout.strip()
    if parser is not None:
        content = parser(completed.stdout)
    if not content:
        raise CLIModelError(f"{spec.label} returned empty output")
    return {"content": content}


def build_cli_env(cli_name: str) -> dict[str, str] | None:
    """Return environment overrides for a specific CLI, if needed."""
    if cli_name != "gemini":
        return None

    compatible_node_bin = find_compatible_node_bin()
    if compatible_node_bin is None:
        return None

    env = os.environ.copy()
    current_path = env.get("PATH", "")
    env["PATH"] = str(compatible_node_bin) + os.pathsep + current_path
    return env


def find_compatible_node_bin() -> Path | None:
    """Prefer a Node runtime that works with the Gemini CLI."""
    candidates = list(candidate_node_bins())
    compatible: list[tuple[tuple[int, ...], Path]] = []

    for bin_dir in candidates:
        node_path = bin_dir / "node"
        version = read_node_version(node_path)
        if version is None:
            continue
        major = version[0]
        if 20 <= major <= 24:
            compatible.append((version, bin_dir))

    if not compatible:
        return None

    compatible.sort(reverse=True)
    return compatible[0][1]


def candidate_node_bins() -> list[Path]:
    """Yield likely Node bin directories in priority order."""
    seen: set[Path] = set()
    candidates: list[Path] = []

    def add(path: Path) -> None:
        resolved = path.expanduser()
        if resolved in seen:
            return
        seen.add(resolved)
        candidates.append(resolved)

    if current_node := shutil.which("node"):
        add(Path(current_node).resolve().parent)

    for pattern in (
        "~/.nvm/versions/node/*/bin",
        "/opt/homebrew/bin",
        "/opt/zerobrew/prefix/bin",
        "/usr/local/bin",
    ):
        for path in sorted(glob(os.path.expanduser(pattern))):
            add(Path(path))

    return candidates


def read_node_version(node_path: Path) -> tuple[int, ...] | None:
    """Return a parsed Node version tuple, or None if unreadable."""
    try:
        completed = subprocess.run(
            [str(node_path), "--version"],
            text=True,
            capture_output=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None

    if completed.returncode != 0:
        return None

    match = re.search(r"v(\d+)\.(\d+)\.(\d+)", completed.stdout.strip())
    if not match:
        return None

    return tuple(int(part) for part in match.groups())


def parse_gemini_response(stdout: str) -> str:
    """Extract the model response from Gemini's noisy JSON mode output."""
    payload = extract_last_json_object(stdout)
    response = payload.get("response")
    if isinstance(response, str) and response.strip():
        return response.strip()

    raise CLIModelError("gemini returned JSON output without a non-empty response field")


def extract_last_json_object(text: str) -> dict[str, object]:
    """Parse the last JSON object embedded in stdout."""
    stripped = text.strip()
    for index in reversed([match.start() for match in re.finditer(r"{", stripped)]):
        candidate = stripped[index:]
        try:
            payload = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(payload, dict):
            return payload
    raise CLIModelError(f"unable to parse gemini JSON output: {tail_output(stripped)}")


def combine_process_output(completed: subprocess.CompletedProcess[str]) -> str:
    """Combine stderr/stdout for readable error reporting."""
    parts = [part.strip() for part in (completed.stderr, completed.stdout) if part and part.strip()]
    return "\n".join(parts)


def tail_output(text: str, *, limit: int = 500) -> str:
    """Return the last `limit` characters from stderr/stdout for readable errors."""
    cleaned = text.strip()
    if len(cleaned) <= limit:
        return cleaned
    return cleaned[-limit:]

#!/usr/bin/env python3
"""Programmatic stage 2 and stage 3 helpers for the llm-council skill."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from collections import defaultdict
from pathlib import Path
from textwrap import dedent
from typing import Any

from cli_client import (
    CLIModelError,
    ModelSpec,
    parse_model_spec,
    query_model,
    query_models_parallel,
)

DEFAULT_RANKERS = [
    "codex:gpt-5.4",
    "claude:opus",
    "gemini:gemini-3.1-pro-preview",
]
DEFAULT_CHAIRMAN = "codex:gpt-5.4"
DEFAULT_MAX_RESPONSE_CHARS = 12000


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run programmatic ranking and chairman synthesis for captured council outputs."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    rank_parser = subparsers.add_parser(
        "rank",
        help="Anonymize captured stage 1 outputs and run programmatic peer ranking.",
    )
    rank_parser.add_argument("--input", required=True, help="Stage 1 JSON artifact path or '-' for stdin.")
    rank_parser.add_argument("--output", default="-", help="Stage 2 JSON artifact path or '-' for stdout.")
    rank_parser.add_argument(
        "--ranker",
        action="append",
        dest="rankers",
        default=[],
        help="Ranking runner in '<cli>:<model>' format. Repeat to supply multiple rankers.",
    )
    rank_parser.add_argument("--timeout", type=float, default=120.0, help="Request timeout in seconds.")
    rank_parser.add_argument(
        "--max-response-chars",
        type=int,
        default=DEFAULT_MAX_RESPONSE_CHARS,
        help="Maximum characters from each captured response to include in prompts.",
    )

    synthesize_parser = subparsers.add_parser(
        "synthesize",
        help="Run the chairman synthesis step using captured stage 1 outputs and stage 2 rankings.",
    )
    synthesize_parser.add_argument("--stage1", required=True, help="Stage 1 JSON artifact path.")
    synthesize_parser.add_argument("--stage2", required=True, help="Stage 2 JSON artifact path.")
    synthesize_parser.add_argument("--output", default="-", help="Stage 3 JSON artifact path or '-' for stdout.")
    synthesize_parser.add_argument(
        "--markdown-output",
        help="Optional path to write the chairman Markdown response only.",
    )
    synthesize_parser.add_argument(
        "--chairman",
        default="",
        help="Chairman runner in '<cli>:<model>' format.",
    )
    synthesize_parser.add_argument("--timeout", type=float, default=120.0, help="Request timeout in seconds.")
    synthesize_parser.add_argument(
        "--max-response-chars",
        type=int,
        default=DEFAULT_MAX_RESPONSE_CHARS,
        help="Maximum characters from each captured response to include in prompts.",
    )

    run_parser = subparsers.add_parser(
        "run",
        help="Run ranking and chairman synthesis in one command.",
    )
    run_parser.add_argument("--input", required=True, help="Stage 1 JSON artifact path or '-' for stdin.")
    run_parser.add_argument("--output", default="-", help="Combined JSON artifact path or '-' for stdout.")
    run_parser.add_argument("--stage2-output", help="Optional path to also write the stage 2 artifact.")
    run_parser.add_argument("--markdown-output", help="Optional path to write the chairman Markdown response.")
    run_parser.add_argument(
        "--ranker",
        action="append",
        dest="rankers",
        default=[],
        help="Ranking runner in '<cli>:<model>' format. Repeat to supply multiple rankers.",
    )
    run_parser.add_argument(
        "--chairman",
        default="",
        help="Chairman runner in '<cli>:<model>' format.",
    )
    run_parser.add_argument("--timeout", type=float, default=120.0, help="Request timeout in seconds.")
    run_parser.add_argument(
        "--max-response-chars",
        type=int,
        default=DEFAULT_MAX_RESPONSE_CHARS,
        help="Maximum characters from each captured response to include in prompts.",
    )

    return parser.parse_args()


def default_rankers() -> list[ModelSpec]:
    configured = split_model_values(os.environ.get("LLM_COUNCIL_RANKERS", ""))
    values = configured or list(DEFAULT_RANKERS)
    return [parse_model_spec(value) for value in values]


def default_chairman() -> ModelSpec:
    configured = os.environ.get("LLM_COUNCIL_CHAIRMAN", "").strip()
    return parse_model_spec(configured or DEFAULT_CHAIRMAN)


def split_model_values(value: str) -> list[str]:
    return [item.strip() for item in value.split(",") if item.strip()]


def read_json_document(path: str) -> tuple[dict[str, Any], Path | None]:
    if path == "-":
        data = json.load(sys.stdin)
        if not isinstance(data, dict):
            raise ValueError("input JSON must be an object")
        return data, None

    file_path = Path(path).expanduser().resolve()
    with file_path.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise ValueError("input JSON must be an object")
    return data, file_path.parent


def write_json_document(path: str, payload: dict[str, Any]) -> None:
    serialized = json.dumps(payload, indent=2, sort_keys=False) + "\n"
    if path == "-":
        sys.stdout.write(serialized)
        return

    file_path = Path(path).expanduser().resolve()
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(serialized, encoding="utf-8")


def write_text_document(path: str, content: str) -> None:
    file_path = Path(path).expanduser().resolve()
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(content.rstrip() + "\n", encoding="utf-8")


def first_nonempty_string(*values: Any) -> str | None:
    for value in values:
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def normalize_files_summary(value: Any) -> str:
    if isinstance(value, str) and value.strip():
        return value.strip()
    if isinstance(value, list):
        items = [item.strip() for item in value if isinstance(item, str) and item.strip()]
        if items:
            preview = ", ".join(items[:5])
            if len(items) > 5:
                preview += f", +{len(items) - 5} more"
            return preview
    return "captured-agent-outputs"


def resolve_text_path(path_value: str, base_dir: Path | None) -> Path:
    candidate = Path(path_value).expanduser()
    if candidate.is_absolute():
        return candidate.resolve()
    if base_dir is not None:
        return (base_dir / candidate).resolve()
    return candidate.resolve()


def normalize_stage1_artifact(raw: dict[str, Any], base_dir: Path | None) -> dict[str, Any]:
    stage1_raw = raw.get("stage1")
    stage1: dict[str, Any] = stage1_raw if isinstance(stage1_raw, dict) else raw
    user_query = first_nonempty_string(
        stage1.get("user_query"),
        stage1.get("query"),
        stage1.get("prompt"),
        stage1.get("request"),
    )
    if user_query is None:
        raise ValueError("stage 1 artifact must include user_query, query, prompt, or request")

    responses_raw = stage1.get("responses")
    if responses_raw is None:
        responses_raw = stage1.get("outputs")
    if responses_raw is None:
        responses_raw = stage1.get("stage1_results")

    if not isinstance(responses_raw, list) or not responses_raw:
        raise ValueError("stage 1 artifact must include a non-empty responses list")

    responses = [
        normalize_response_record(item, index + 1, base_dir)
        for index, item in enumerate(responses_raw)
    ]

    files_summary_value = stage1.get("files_summary", stage1.get("files"))

    return {
        "schema_version": 1,
        "user_query": user_query,
        "review_goal": first_nonempty_string(stage1.get("review_goal"), stage1.get("goal")),
        "context": first_nonempty_string(stage1.get("context"), stage1.get("code_context")),
        "files_summary": normalize_files_summary(files_summary_value),
        "language": first_nonempty_string(stage1.get("language"), stage1.get("lang")) or "Unknown",
        "responses": responses,
    }


def normalize_response_record(item: Any, index: int, base_dir: Path | None) -> dict[str, Any]:
    if not isinstance(item, dict):
        raise ValueError(f"response {index} must be a JSON object")

    source = first_nonempty_string(
        item.get("source"),
        item.get("agent"),
        item.get("name"),
        item.get("reviewer"),
    ) or f"Reviewer {index}"
    model = first_nonempty_string(
        item.get("model"),
        item.get("source_model"),
        item.get("reviewer_model"),
    )
    content = first_nonempty_string(
        item.get("content"),
        item.get("response"),
        item.get("text"),
        item.get("output"),
    )

    path_value = first_nonempty_string(
        item.get("path"),
        item.get("file"),
        item.get("output_path"),
    )
    resolved_path: Path | None = None
    if content is None and path_value is not None:
        resolved_path = resolve_text_path(path_value, base_dir)
        content = resolved_path.read_text(encoding="utf-8")

    if content is None:
        raise ValueError(
            f"response {index} must include content/response/text/output or a path/file/output_path"
        )
    if not content.strip():
        raise ValueError(f"response {index} content is empty")

    normalized = {
        "source": source,
        "content": content.strip(),
    }
    if model is not None:
        normalized["model"] = model
    if resolved_path is not None:
        normalized["path"] = str(resolved_path)
    elif path_value is not None:
        normalized["path"] = str(resolve_text_path(path_value, base_dir))
    return normalized


def normalize_stage2_artifact(raw: dict[str, Any]) -> dict[str, Any]:
    stage2_raw = raw.get("stage2")
    stage2: dict[str, Any] = stage2_raw if isinstance(stage2_raw, dict) else raw
    if not isinstance(stage2.get("rankings"), list):
        raise ValueError("stage 2 artifact must include rankings")
    return stage2


def response_label(index: int) -> str:
    number = index + 1
    parts: list[str] = []
    while number > 0:
        number, remainder = divmod(number - 1, 26)
        parts.append(chr(65 + remainder))
    return f"Response {''.join(reversed(parts))}"


def build_identity_aliases(responses: list[dict[str, Any]]) -> list[str]:
    aliases: set[str] = set()
    for response in responses:
        for key in ("source", "model"):
            value = response.get(key)
            if not isinstance(value, str) or len(value.strip()) < 3:
                continue
            aliases.add(value.strip())
            if "/" in value:
                aliases.add(value.split("/", 1)[1].strip())
    return sorted(aliases, key=lambda alias: len(alias), reverse=True)


def redact_identities(text: str, aliases: list[str]) -> str:
    redacted = text
    for alias in aliases:
        pattern = re.compile(re.escape(alias), re.IGNORECASE)
        redacted = pattern.sub("[REDACTED]", redacted)
    return redacted


def truncate_text(text: str, limit: int) -> str:
    cleaned = text.strip()
    if limit <= 0 or len(cleaned) <= limit:
        return cleaned
    return cleaned[:limit].rstrip() + "\n\n[TRUNCATED]"


def anonymize_responses(
    responses: list[dict[str, Any]],
    *,
    max_response_chars: int,
) -> list[dict[str, Any]]:
    aliases = build_identity_aliases(responses)
    anonymized: list[dict[str, Any]] = []

    for index, response in enumerate(responses):
        label = response_label(index)
        content = redact_identities(response["content"], aliases)
        anonymized.append(
            {
                "label": label,
                "source": response["source"],
                "source_model": response.get("model"),
                "content": truncate_text(content, max_response_chars),
            }
        )

    return anonymized


def build_stage2_prompt(stage1: dict[str, Any], anonymized_responses: list[dict[str, Any]]) -> str:
    review_goal = stage1.get("review_goal")
    context = stage1.get("context")
    extra_sections: list[str] = []
    if review_goal:
        extra_sections.append(f"Review goal:\n{review_goal}")
    if context:
        extra_sections.append(f"Workspace context:\n{context}")

    responses_text = "\n\n".join(
        f"{response['label']}:\n{response['content']}" for response in anonymized_responses
    )

    extra_context = "\n\n".join(extra_sections)
    if extra_context:
        extra_context = "\n\n" + extra_context

    return dedent(
        f"""
        You are evaluating anonymized code review responses for the same request.

        Original review request:
        {stage1['user_query']}{extra_context}

        Rank the responses using these criteria, in order:
        1. Correctness and low false-positive rate
        2. Concrete evidence such as file paths, line references, or code-path reasoning
        3. Actionability and prioritization
        4. Coverage of the most important risks without unnecessary noise
        5. Clarity and usefulness to an engineer fixing the code

        For each response:
        - Summarize strengths in 1-2 sentences
        - Summarize weaknesses in 1-2 sentences
        - Refer to responses only by label

        Then provide a final ranking with no ties.

        IMPORTANT: Your final ranking MUST be formatted EXACTLY as follows:
        FINAL RANKING:
        1. Response A
        2. Response B
        3. Response C

        Responses:

        {responses_text}
        """
    ).strip()


def parse_ranking_from_text(ranking_text: str) -> list[str]:
    if "FINAL RANKING:" in ranking_text:
        _, ranking_section = ranking_text.split("FINAL RANKING:", 1)
        numbered_matches = re.findall(r"\d+\.\s*Response [A-Z]+", ranking_section)
        if numbered_matches:
            parsed_matches: list[str] = []
            for item in numbered_matches:
                match = re.search(r"Response [A-Z]+", item)
                if match is not None:
                    parsed_matches.append(match.group(0))
            if parsed_matches:
                return parsed_matches
        matches = re.findall(r"Response [A-Z]+", ranking_section)
        if matches:
            return matches
    return re.findall(r"Response [A-Z]+", ranking_text)


def calculate_aggregate_rankings(
    anonymized_responses: list[dict[str, Any]],
    rankings: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    positions: dict[str, list[int]] = defaultdict(list)
    first_place_votes: dict[str, int] = defaultdict(int)

    for ranking in rankings:
        for position, label in enumerate(ranking.get("parsed_ranking", []), start=1):
            positions[label].append(position)
            if position == 1:
                first_place_votes[label] += 1

    aggregate: list[dict[str, Any]] = []
    for response in anonymized_responses:
        label = response["label"]
        response_positions = positions.get(label, [])
        average_rank = (
            round(sum(response_positions) / len(response_positions), 2)
            if response_positions
            else None
        )
        aggregate.append(
            {
                "label": label,
                "source": response["source"],
                "source_model": response.get("source_model"),
                "average_rank": average_rank,
                "rankings_count": len(response_positions),
                "first_place_votes": first_place_votes.get(label, 0),
            }
        )

    aggregate.sort(
        key=lambda item: (
            item["average_rank"] is None,
            item["average_rank"] if item["average_rank"] is not None else 999,
            -item["first_place_votes"],
            item["label"],
        )
    )
    return aggregate


def run_stage2(
    stage1: dict[str, Any],
    *,
    rankers: list[ModelSpec],
    timeout: float,
    max_response_chars: int,
) -> dict[str, Any]:
    anonymized_responses = anonymize_responses(
        stage1["responses"],
        max_response_chars=max_response_chars,
    )
    prompt = build_stage2_prompt(stage1, anonymized_responses)
    ranker_results = query_models_parallel(
        rankers,
        prompt,
        timeout=timeout,
    )

    rankings: list[dict[str, Any]] = []
    for result in ranker_results:
        content = result.get("content")
        entry: dict[str, Any] = {
            "reviewer_cli": result["cli"],
            "reviewer_model": result["model"],
            "reviewer_spec": f"{result['cli']}:{result['model']}",
            "ranking_text": content,
            "parsed_ranking": [],
            "error": result.get("error"),
        }
        if isinstance(content, str):
            entry["parsed_ranking"] = parse_ranking_from_text(content)
        rankings.append(entry)

    aggregate_rankings = calculate_aggregate_rankings(anonymized_responses, rankings)

    return {
        "schema_version": 1,
        "user_query": stage1["user_query"],
        "review_goal": stage1.get("review_goal"),
        "context": stage1.get("context"),
        "files_summary": stage1.get("files_summary"),
        "language": stage1.get("language"),
        "anonymized_responses": anonymized_responses,
        "label_to_source": {
            response["label"]: response["source"] for response in anonymized_responses
        },
        "label_to_model": {
            response["label"]: response.get("source_model") for response in anonymized_responses
        },
        "rankings": rankings,
        "aggregate_rankings": aggregate_rankings,
    }


def build_aggregate_summary(stage2: dict[str, Any]) -> str:
    rows: list[str] = []
    for item in stage2.get("aggregate_rankings", []):
        model_suffix = f" [{item['source_model']}]" if item.get("source_model") else ""
        avg_rank = item["average_rank"] if item["average_rank"] is not None else "n/a"
        rows.append(
            f"- {item['label']} -> {item['source']}{model_suffix}: "
            f"avg_rank={avg_rank}, first_place_votes={item['first_place_votes']}, "
            f"rankings_count={item['rankings_count']}"
        )
    if not rows:
        return "- No successful rankings were captured."
    return "\n".join(rows)


def build_stage3_prompt(
    stage1: dict[str, Any],
    stage2: dict[str, Any],
    *,
    max_response_chars: int,
) -> str:
    review_goal = stage1.get("review_goal")
    context = stage1.get("context")
    stage1_sections: list[str] = []
    for response in stage1["responses"]:
        header = response["source"]
        if response.get("model"):
            header += f" ({response['model']})"
        stage1_sections.append(
            f"### {header}\n{truncate_text(response['content'], max_response_chars)}"
        )

    ranking_sections: list[str] = []
    for ranking in stage2.get("rankings", []):
        heading = f"### {ranking['reviewer_model']}"
        if ranking.get("error"):
            ranking_sections.append(f"{heading}\nERROR: {ranking['error']}")
            continue
        ranking_sections.append(
            f"{heading}\n{truncate_text(ranking.get('ranking_text') or '', max_response_chars // 2)}"
        )

    review_goal_block = f"\nReview goal:\n{review_goal}\n" if review_goal else ""
    context_block = f"\nWorkspace context:\n{context}\n" if context else ""

    return dedent(
        f"""
        You are the Chairman of a code review council. Multiple reviewer outputs were captured,
        then ranked through an anonymized ranking stage. Synthesize them into one final review.

        Rules:
        - Use only evidence that appears in the captured outputs below.
        - Do not invent bugs, code snippets, file paths, or line numbers.
        - Merge duplicates and prioritize issues mentioned by multiple reviewers.
        - If reviewers disagree, mention the disagreement briefly instead of flattening it away.
        - Keep the final report concise and engineer-friendly.

        Original review request:
        {stage1['user_query']}
        {review_goal_block}{context_block}
        Files summary: {stage1.get('files_summary') or 'captured-agent-outputs'}
        Language: {stage1.get('language') or 'Unknown'}

        Stage 1 - Captured reviewer outputs:
        {chr(10).join(stage1_sections)}

        Stage 2 - Anonymized rankings:
        Aggregate ranking summary:
        {build_aggregate_summary(stage2)}

        Detailed ranking outputs:
        {chr(10).join(ranking_sections) if ranking_sections else 'No successful rankings were captured.'}

        Produce the final answer in Markdown using this structure:

        ## Code Review Council Report
        ### Summary
        ### Critical Issues
        ### Recommended Improvements
        ### Minor Suggestions
        ### Strengths

        Include source attribution inline when it adds clarity, for example:
        "Flagged by: Codex, Gemini"
        """
    ).strip()


def run_stage3(
    stage1: dict[str, Any],
    stage2: dict[str, Any],
    *,
    chairman: ModelSpec,
    timeout: float,
    max_response_chars: int,
) -> dict[str, Any]:
    prompt = build_stage3_prompt(stage1, stage2, max_response_chars=max_response_chars)
    response = query_model(chairman, prompt, timeout=timeout)
    return {
        "schema_version": 1,
        "chairman_cli": chairman.cli,
        "chairman_model": chairman.model,
        "chairman_spec": chairman.label,
        "response": response["content"],
    }


def handle_rank(args: argparse.Namespace) -> int:
    raw_stage1, base_dir = read_json_document(args.input)
    stage1 = normalize_stage1_artifact(raw_stage1, base_dir)
    rankers = [parse_model_spec(value) for value in args.rankers] if args.rankers else default_rankers()
    stage2 = run_stage2(
        stage1,
        rankers=rankers,
        timeout=args.timeout,
        max_response_chars=args.max_response_chars,
    )
    write_json_document(args.output, stage2)
    return 0


def handle_synthesize(args: argparse.Namespace) -> int:
    raw_stage1, base_dir = read_json_document(args.stage1)
    stage1 = normalize_stage1_artifact(raw_stage1, base_dir)
    raw_stage2, _ = read_json_document(args.stage2)
    stage2 = normalize_stage2_artifact(raw_stage2)
    chairman = parse_model_spec(args.chairman) if args.chairman else default_chairman()
    stage3 = run_stage3(
        stage1,
        stage2,
        chairman=chairman,
        timeout=args.timeout,
        max_response_chars=args.max_response_chars,
    )
    write_json_document(args.output, stage3)
    if args.markdown_output:
        write_text_document(args.markdown_output, stage3["response"])
    return 0


def handle_run(args: argparse.Namespace) -> int:
    raw_stage1, base_dir = read_json_document(args.input)
    stage1 = normalize_stage1_artifact(raw_stage1, base_dir)
    rankers = [parse_model_spec(value) for value in args.rankers] if args.rankers else default_rankers()
    chairman = parse_model_spec(args.chairman) if args.chairman else default_chairman()

    stage2 = run_stage2(
        stage1,
        rankers=rankers,
        timeout=args.timeout,
        max_response_chars=args.max_response_chars,
    )
    stage3 = run_stage3(
        stage1,
        stage2,
        chairman=chairman,
        timeout=args.timeout,
        max_response_chars=args.max_response_chars,
    )

    combined = {
        "schema_version": 1,
        "stage1": stage1,
        "stage2": stage2,
        "stage3": stage3,
    }

    write_json_document(args.output, combined)
    if args.stage2_output:
        write_json_document(args.stage2_output, stage2)
    if args.markdown_output:
        write_text_document(args.markdown_output, stage3["response"])
    return 0


def main() -> int:
    args = parse_args()
    try:
        if args.command == "rank":
            return handle_rank(args)
        if args.command == "synthesize":
            return handle_synthesize(args)
        if args.command == "run":
            return handle_run(args)
        raise ValueError(f"unsupported command: {args.command}")
    except (CLIModelError, OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

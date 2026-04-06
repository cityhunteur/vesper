#!/usr/bin/env python3
"""Query a local model CLI and print the response text."""
from __future__ import annotations

import argparse
import sys

from cli_client import CLIModelError, parse_model_spec, query_model


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Query a single model CLI.")
    parser.add_argument(
        "--runner",
        default="codex:gpt-5.4",
        help="Runner in '<cli>:<model>' format. Default: codex:gpt-5.4",
    )
    parser.add_argument("prompt", help="Prompt to send as a single user message.")
    parser.add_argument(
        "--timeout",
        type=float,
        default=120.0,
        help="Request timeout in seconds.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    try:
        result = query_model(parse_model_spec(args.runner), args.prompt, timeout=args.timeout)
    except CLIModelError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)

    print(result["content"])


if __name__ == "__main__":
    main()

# Programmatic Council Pipeline

Use the helper scripts in `skills/llm-council/scripts/` to turn captured stage 1 outputs into:
- a programmatic anonymized ranking stage
- a real chairman synthesis step that consumes the captured outputs

## CLI Models Used

Stage 1 reviewer CLIs:
- `codex:gpt-5.4`
- `claude:opus`
- `gemini:gemini-3.1-pro-preview`

Stage 2 ranker defaults:
- `codex:gpt-5.4`
- `claude:opus`
- `gemini:gemini-3.1-pro-preview`

Stage 3 chairman default:
- `codex:gpt-5.4`

Notes:
- The pipeline uses local CLIs, not OpenRouter HTTP calls.
- For PR reviews, prefer `codex exec review --base <exact-base-sha>` from a checkout/worktree at the PR head.
- Generic Codex runner profile: `codex exec --model gpt-5.4 --config model_reasoning_effort="xhigh"`
- Claude runner profile: `claude -p --model opus`
- Gemini runner profile: `env PATH="$HOME/.nvm/versions/node/v22.7.0/bin:$PATH" gemini --model gemini-3.1-pro-preview --output-format json -p`
- Set `LLM_COUNCIL_ALLOW_UNSAFE_CLI=1` only if a trusted local environment requires legacy `--yolo` or `--dangerously-*` flags.
- `gemini` is best-effort. If the default Node runtime is too new for the installed Gemini CLI, prefer a local Node 20-24 runtime (the helper scripts now do this automatically).

## Preferred Stage 1 Capture

Use the helper script to collect stage 1 outputs:

```bash
python3 ./skills/llm-council/scripts/stage1_capture.py \
  --repo /path/to/repo \
  --head-rev <PR_HEAD_SHA_OR_REF> \
  --base-rev <PR_BASE_SHA> \
  --context-file /tmp/llm-council-context.md \
  --user-query "Review this code for issues" \
  --output /tmp/llm-council-stage1.json \
  --files-summary "pkg/handler/handler.go (1 file, +42 -9)" \
  --language "Go"
```

This script:
- uses a temporary worktree for PR-head reviews when `--head-rev` is provided
- runs Codex in repo-aware `review` mode against the exact base revision
- selects a compatible Node runtime for Gemini automatically
- writes a ready-to-rank stage 1 artifact

## Stage 1 Artifact

Save stage 1 reviewer outputs into a JSON artifact. Each response can inline `content` or point at a captured output file with `path`.

```json
{
  "user_query": "Review this code for issues",
  "files_summary": "pkg/handler/handler.go (1 file, +42 -9)",
  "language": "Go",
  "responses": [
    {
      "source": "Codex",
      "model": "gpt-5.4",
      "path": "/tmp/codex-review-output.md"
    },
    {
      "source": "Claude",
      "model": "opus",
      "path": "/tmp/claude-review-output.md"
    },
    {
      "source": "Gemini",
      "model": "gemini-3.1-pro-preview",
      "path": "/tmp/gemini-review-output.md"
    },
    {
      "source": "Manual",
      "content": "My own notes about a schema mismatch in events.go..."
    }
  ]
}
```

Suggested path:

```bash
/tmp/llm-council-stage1.json
```

## Stage 2: Ranking

Generate anonymized labels, redact visible model identities where possible, ask ranker models to score the responses, and save the parsed ranking artifact:

```bash
python3 skills/llm-council/scripts/council_pipeline.py rank \
  --input /tmp/llm-council-stage1.json \
  --output /tmp/llm-council-stage2.json
```

Defaults:
- Rankers come from `LLM_COUNCIL_RANKERS` if set, otherwise:
  - `codex:gpt-5.4`
  - `claude:opus`
  - `gemini:gemini-3.1-pro-preview`
- Override by repeating `--ranker <cli>:<model>`

The stage 2 artifact contains:
- anonymized labels (`Response A`, `Response B`, ...)
- label-to-source/model mapping
- captured ranking text per ranker
- parsed `FINAL RANKING` order
- aggregate rankings (average rank + first-place votes)

## Stage 3: Chairman Synthesis

Generate the synthesized report from the captured stage 1 outputs and the stage 2 ranking artifact:

```bash
python3 skills/llm-council/scripts/council_pipeline.py synthesize \
  --stage1 /tmp/llm-council-stage1.json \
  --stage2 /tmp/llm-council-stage2.json \
  --output /tmp/llm-council-stage3.json \
  --markdown-output /tmp/llm-council-final.md
```

Defaults:
- Chairman comes from `LLM_COUNCIL_CHAIRMAN` if set, otherwise `codex:gpt-5.4`
- Override with `--chairman <cli>:<model>`

## Combined Run

To do ranking and synthesis in one command:

```bash
python3 skills/llm-council/scripts/council_pipeline.py run \
  --input /tmp/llm-council-stage1.json \
  --output /tmp/llm-council-council.json \
  --stage2-output /tmp/llm-council-stage2.json \
  --markdown-output /tmp/llm-council-final.md
```

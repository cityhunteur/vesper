---
name: llm-council
description: Multi-perspective code review council that analyzes code from seven expert viewpoints (Security, Performance, Readability, Architecture, Testing, Maintainability, Go Idioms) and can orchestrate Codex, Claude, and Gemini for deeper analysis. Use when the user asks for a code review, feedback on an implementation, says "review this code", "what do you think of this implementation", "give me feedback on this code", "council review", "multi-agent review", "analyze this for issues", "is this idiomatic Go", or asks whether code follows Go best practices.
---

# LLM Council

A code review council that examines code through seven expert lenses, leveraging multiple LLM coding agents in parallel, and synthesizing findings into a single prioritized report.

## Activation Rules

- Use council mode for requests to review, assess, critique, analyze, or give feedback on existing code or an implementation.
- Strong triggers include: `Review this code`, `Council review on this file`, `What do you think of this implementation?`, `Give me feedback on this code`, `Analyze this for issues`, `Code review please`, `Is this idiomatic Go?`, `Check if this follows Go best practices`, and `Multi-agent review on this file`.
- Do not use council mode for requests to write code, refactor, run tests, deploy, create a PR, or explain an error unless the user also asks for review.
- If no file path is given, inspect the current diff/status or recently edited files first. Ask one concise follow-up only if no plausible review target can be inferred.

## Council Personas

The council consists of seven expert reviewers:

| Persona | Focus Areas |
|---------|-------------|
| **Security Expert** | Injection vulnerabilities, auth flaws, data exposure, input validation, secrets handling |
| **Performance Expert** | Time complexity, memory usage, I/O efficiency, caching opportunities, bottlenecks |
| **Readability Expert** | Naming clarity, code structure, comments, self-documenting patterns, cognitive load |
| **Architecture Expert** | SOLID principles, separation of concerns, coupling, cohesion, design patterns |
| **Testing Expert** | Testability, edge cases, error paths, mocking needs, coverage gaps |
| **Maintainability Expert** | Technical debt, change impact, documentation, upgrade paths, dependency health |
| **Go Idioms Expert** | Idiomatic Go patterns, effective Go practices, Go proverbs adherence, standard library usage |

## Go Idioms Expert Focus

When reviewing Go code, the Go Idioms Expert specifically checks for:

### Naming Conventions
- Package names: short, lowercase, no underscores (e.g., `strconv` not `string_conversion`)
- MixedCaps for exported, mixedCaps for unexported
- Getters without "Get" prefix (e.g., `obj.Owner()` not `obj.GetOwner()`)
- Interface names: single-method interfaces end in `-er` (e.g., `Reader`, `Writer`, `Stringer`)
- Avoid stuttering (e.g., `http.Server` not `http.HTTPServer`)

### Error Handling
- Return errors as the last return value
- Handle errors explicitly, don't ignore with `_`
- Wrap errors with context using `fmt.Errorf("context: %w", err)`
- Use sentinel errors or error types appropriately
- Check `errors.Is()` and `errors.As()` for error comparison

### Code Structure
- Accept interfaces, return structs
- Keep interfaces small (1-3 methods)
- Use functional options for complex configuration
- Prefer composition over inheritance
- Use embedding judiciously

### Concurrency
- Share memory by communicating (channels over shared state)
- Don't communicate by sharing memory
- Proper goroutine lifecycle management
- Use `context.Context` for cancellation
- Avoid goroutine leaks

### Standard Patterns
- Use `defer` for cleanup
- Table-driven tests
- Use `io.Reader`/`io.Writer` interfaces
- Prefer `sync.Mutex` over channels for simple state protection
- Use `sync.Once` for one-time initialization

### Go Proverbs
- "A little copying is better than a little dependency"
- "Clear is better than clever"
- "The bigger the interface, the weaker the abstraction"
- "Make the zero value useful"
- "Don't just check errors, handle them gracefully"

## Workspace Detection & Strategy

Before launching agents, determine the workspace type. This affects how agents are invoked and how context is gathered.

### Workspace Types

| Type | Detection | Strategy |
|------|-----------|----------|
| **Single repo** | `git rev-parse --git-dir` succeeds in CWD | Standard: run agents from CWD |
| **Monorepo** | Single `.git` at root, multiple packages/services underneath | Standard: run agents from repo root |
| **Multi-repo workspace** | CWD is NOT a git repo, but contains multiple git repos as subdirectories | Special handling required (see below) |

### Multi-Repo Workspace Handling

**Problem**: Tools like `codex exec` require running from inside a git repository. In a multi-repo workspace (e.g., `~/workspace/` containing `repo-a/`, `repo-b/`, `repo-c/`), the workspace root is NOT a git repo.

**Strategy**:

1. **Detect**: Run `git rev-parse --git-dir 2>/dev/null` — if it fails, you're in a multi-repo workspace
2. **Identify target repos**: Determine which repos are affected by the review (may span 1-N repos)
3. **Gather context upfront**: Save diffs and key file contents to a temporary context file before launching agents
4. **Pick a primary repo**: For `codex exec`, `cd` into the most relevant repo (usually the one with the most changes)
5. **Use context files**: Pass the pre-gathered context file to agents so they can review cross-repo changes

**Context file pattern** (save to `/tmp/llm-council-context.md`):

```markdown
# Code Review Context

## Repos Under Review
- repo-a (branch: feat/xyz) — N files changed, +X -Y lines
- repo-b (branch: feat/abc) — N files changed, +X -Y lines

## Diffs
### repo-a
\`\`\`diff
<git diff main..HEAD output>
\`\`\`

### repo-b
\`\`\`diff
<git diff main..HEAD output>
\`\`\`

## Key Files (full content for critical files)
### repo-a/path/to/important_file.go
\`\`\`go
<file contents>
\`\`\`
```

**Important**: Keep the context file under 50KB to avoid agent timeouts. For large diffs, include only the most critical files in full and summarize the rest.

## Multi-Agent Orchestration

The council leverages multiple LLM coding agents running in parallel for comprehensive analysis. Each agent brings different training data and perspectives.

### Agent Configuration

Run all three agents **in parallel** using background tasks.

**Preferred stage 1 runner**:

Use `python3 ./skills/llm-council/scripts/stage1_capture.py` when possible. It fixes two common failure modes and now defaults to safer CLI settings:
- **Codex PR drift**: for PR reviews, it checks out the exact head revision in a temporary worktree and runs `codex exec review --base <exact-base-sha>` so Codex reviews the PR delta instead of unrelated branch drift.
- **Gemini Node incompatibility**: it automatically prefers a compatible local Node 20-24 runtime for the installed Gemini CLI. This avoids the `SlowBuffer.prototype` crash seen under Node 25.

Example:

```bash
python3 ./skills/llm-council/scripts/stage1_capture.py \
  --repo /path/to/repo \
  --head-rev <PR_HEAD_SHA_OR_REF> \
  --base-rev <PR_BASE_SHA> \
  --context-file /tmp/llm-council-context.md \
  --user-query "Review this PR for issues" \
  --output /tmp/llm-council-stage1.json \
  --files-summary "path/to/file.proto (1 file, +84 -0)" \
  --language "Protocol Buffers"
```

**IMPORTANT — Agent launch requirements**:
- **Codex**: Use `codex exec --model gpt-5.4 --config model_reasoning_effort="xhigh"`.
- **Claude**: Use `claude -p --model opus`.
- **Gemini**: Use `gemini --model gemini-3.1-pro-preview -p`. In some environments the installed Gemini CLI may be broken; treat it as best-effort and continue if it fails.
- Set `LLM_COUNCIL_ALLOW_UNSAFE_CLI=1` only in a trusted environment if legacy `--yolo` or `--dangerously-*` behavior is truly required.

```bash
# Agent 1: OpenAI Codex — prefer repo-aware review mode for PRs
# MUST run from a checkout of the PR head, and use the exact PR base SHA.
cd <PR_HEAD_WORKTREE> && codex exec review \
  --base <PR_BASE_SHA> \
  --model gpt-5.4 \
  --config model_reasoning_effort="xhigh" \
  --ephemeral \
  -o /tmp/codex-review-output.md
```

```bash
# Agent 2: Claude (Opus) — CLI
claude -p \
  --model opus \
  "Review this code for issues, focusing on architecture, maintainability, and Go idioms. Read these files: <FILE_PATHS>. Report findings as a structured list with severity, file:line, and description." \
  > /tmp/claude-review-output.md
```

```bash
# Agent 3: Gemini (3.1 Pro Preview) — CLI (best-effort; continue on failure)
env PATH="$HOME/.nvm/versions/node/v22.7.0/bin:$PATH" \
gemini --model gemini-3.1-pro-preview --output-format json -p \
  "Review this code for issues, focusing on testing, readability, and best practices. Read <CONTEXT_FILE>"
```

### Agent Responsibilities

| Agent | Model | Focus Areas |
|-------|-------|-------------|
| **Codex** | gpt-5.4 | Security vulnerabilities, performance bottlenecks, idiomatic patterns |
| **Claude** | opus | Architecture, maintainability, Go idioms, error handling |
| **Gemini** | gemini-3.1-pro-preview | Testing coverage, readability, best practices, documentation |

### Agent Failure Modes & Fallbacks

| Agent | Common Failure | Fallback |
|-------|----------------|----------|
| **Codex** | "Not inside a trusted directory" | `cd` into a git repo first. Only use `LLM_COUNCIL_ALLOW_UNSAFE_CLI=1` for legacy skip checks in a trusted environment. |
| **Codex** | Empty output / exit 0 | Check `-o` output file; if empty, verify prompt wasn't too short |
| **Claude** | CLI invocation fails or lacks tool access | Include what was returned; supplement with manual analysis |
| **Gemini** | CLI crashes or model unavailable | Continue with Codex + Claude + manual analysis |
| **Any agent** | Fails entirely | Continue with remaining agents + manual seven-perspective analysis |

**Resilience rule**: If any agent fails, do NOT block the review. Proceed with available agents and supplement with manual analysis. A review from 2 agents + manual analysis is still valuable.

### Captured Stage 1 Artifact

After the stage 1 reviewers finish, save their outputs into `/tmp/llm-council-stage1.json`.
The helper scripts accept either inline `content` or file-based `path`.
Detailed artifact format and command examples live in [references/programmatic-pipeline.md](references/programmatic-pipeline.md).

### Programmatic Stage 2: Anonymized Peer Ranking

Use the ranking helper to anonymize captured responses, redact visible model identities where possible, and ask ranking models to score the responses:

```bash
python3 skills/llm-council/scripts/council_pipeline.py rank \
  --input /tmp/llm-council-stage1.json \
  --output /tmp/llm-council-stage2.json
```

See [references/programmatic-pipeline.md](references/programmatic-pipeline.md) for CLI model defaults and the generated artifact shape.

### Programmatic Stage 3: Chairman Synthesis

Use the chairman helper to consume the captured stage 1 outputs plus the stage 2 ranking artifact and produce a real synthesized review:

```bash
python3 skills/llm-council/scripts/council_pipeline.py synthesize \
  --stage1 /tmp/llm-council-stage1.json \
  --stage2 /tmp/llm-council-stage2.json \
  --output /tmp/llm-council-stage3.json \
  --markdown-output /tmp/llm-council-final.md
```

See [references/programmatic-pipeline.md](references/programmatic-pipeline.md) for chairman defaults and the one-command `run` mode.

### Combined Programmatic Run

To do both steps in one command after stage 1 capture:

```bash
python3 skills/llm-council/scripts/council_pipeline.py run \
  --input /tmp/llm-council-stage1.json \
  --output /tmp/llm-council-council.json \
  --stage2-output /tmp/llm-council-stage2.json \
  --markdown-output /tmp/llm-council-final.md
```

### Execution Strategy

1. **Detect workspace type** (single repo, monorepo, multi-repo)
2. **Gather context** — diffs, key files, save to context file if multi-repo
3. **Launch all agents in parallel** using background bash tasks
4. **Conduct manual seven-perspective analysis** while agents run
5. **Capture stage 1 outputs** into `/tmp/llm-council-stage1.json`
6. **Run `council_pipeline.py rank`** to produce anonymized stage 2 rankings
7. **Run `council_pipeline.py synthesize`** (or `run`) to produce the chairman report
8. **Deduplicate and prioritize** overlapping issues using the synthesized output

When multiple agents flag the same issue, it increases the priority/confidence of that finding.

## Critical Review Practices

### Data Flow Tracing (MANDATORY)

**Lesson Learned:** Surface-level reviews miss semantic bugs. You MUST trace data flow end-to-end.

When reviewing code that involves:
- **Serialization/Deserialization**: Trace from the type declaration to actual marshaling code
- **Event Publishing**: Verify the declared event type matches what the marshaler actually emits
- **Schema Registry**: Confirm header metadata matches payload schema
- **Database Migrations**: Test SQL logic with actual data values, not just syntax

**Example of missed bug:**
```go
// usecase.go - declares proto message type
ProtoMessage: "ocos.product.v1alpha1.ProductActivatedEvent",

// events.go - BUT marshaler emits different type!
case product.Activated:
    return proto.Marshal(&pb.ProductUpdatedEvent{...})  // MISMATCH!
```

**Review checklist for event/message systems:**
1. For each `ProtoMessage` value, find the corresponding marshaler and verify types match
2. For SQL transformations (INITCAP, string manipulation), test with actual values
3. For schema-dependent code, trace: declaration → serialization → deserialization → consumption

### SQL Migration Verification

When reviewing migrations that transform data:
- **INITCAP pitfall**: `INITCAP('default_version_set')` produces `Default_Version_Set`, not `DefaultVersionSet`
- **Test with real values**: Don't just review syntax; mentally execute with actual `event_type` values
- **Check all segments**: If SQL processes multiple parts (e.g., `split_part(..., 1)` and `split_part(..., 2)`), verify transformation logic for EACH segment

## Review Workflow

### Step 1: Gather Code Context

Identify the code to review:
- Read the file(s) specified by the user
- Understand the broader context (what the code does, its role in the system)
- Note the language, framework, and any relevant conventions
- **For Go code**: Check for go.mod, identify the module path, and note Go version
- **For event-driven systems**: Identify marshalers, schema definitions, and consumers
- **Detect workspace type**: Run `git rev-parse --git-dir 2>/dev/null` to check if CWD is a git repo
- **For multi-repo reviews**: Gather diffs from all affected repos, identify key files, save to `/tmp/llm-council-context.md`
- **For broad review prompts**: inspect `git diff --stat`, `git status --short`, or the named files first so the report reviews real code instead of answering generically

### Step 2: Choose Fast Path vs Multi-Agent Enrichment

- Start the manual seven-perspective council review immediately for every in-scope review request.
- Launch the external agents only when the review target is clear or the user explicitly asks for council or multi-agent review.
- If agents are slow or fail, do NOT block the report; continue with the manual council report and note which agent outputs are unavailable.
- For explicit multi-agent review requests, still include `Codex`, `Claude`, and `Gemini` in the final report even if one or more timed out.

Preferred agents when enrichment is warranted:
1. **Codex** — `codex exec` CLI
2. **Claude** — `claude -p --model opus`
3. **Gemini** — `gemini --model gemini-3.1-pro-preview -p` on a best-effort basis

### Step 3: Conduct Seven-Perspective Analysis

While agents run, also analyze the code from each persona's viewpoint. For each persona, identify:
- **Issues**: Problems that should be fixed
- **Warnings**: Potential concerns worth noting
- **Suggestions**: Improvements that would enhance quality

### Step 4: Aggregate Agent Results

Collect outputs from all three agents and the stage 2 artifact:
- Identify issues flagged by multiple agents (higher confidence)
- Use `aggregate_rankings` from `/tmp/llm-council-stage2.json` to spot strong/weak responses
- Extract unique insights from each agent
- Note any conflicting recommendations

### Step 5: Synthesize Unified Report

Prefer using the chairman synthesis helper so the report is generated from captured stage 1 plus stage 2 data:

```bash
python3 skills/llm-council/scripts/council_pipeline.py synthesize \
  --stage1 /tmp/llm-council-stage1.json \
  --stage2 /tmp/llm-council-stage2.json \
  --markdown-output /tmp/llm-council-final.md
```

The chairman output should always use these sections in order: `Summary`, `Critical`, `High`, `Medium`, `Low`, `Strengths`, `Agent Consensus`.

## Priority Classification

Classify each finding by severity:

| Priority | Criteria | Examples |
|----------|----------|----------|
| **Critical** | Security risk, data loss potential, crashes, schema mismatches | SQL injection, unhandled null, race condition, **header/payload type mismatch** |
| **High** | Significant quality/performance impact, data integrity | O(n²) where O(n) possible, tight coupling, **event type declared but different type serialized** |
| **Medium** | Maintainability or clarity concern | Poor naming, missing error handling |
| **Low** | Minor improvement opportunity | Style inconsistency, minor optimization |

### Header/Payload Mismatch (Critical)

A particularly insidious bug class: when metadata (headers, type fields) declares one type but the actual payload contains a different type. This breaks:
- Schema registry lookups
- Deserialization on consumers
- Type-safe event handling

**Always verify:** declared type == serialized type == consumed type

## Output Format

**CRITICAL — Emoji and Formatting Rules:**

- For any in-scope review, always start with the exact heading `## 📋 Code Review Council Report`.
- Always include the literal severity words `Critical`, `High`, `Medium`, and `Low` as headings, even if the section only says `No Critical issues found.`
- Always include the literal area names `Security`, `Performance`, `Readability`, `Architecture`, `Testing`, `Maintainability`, and `Go Idioms` somewhere in the report. If the language is not Go, write `Go Idioms: N/A for this language` instead of omitting it.
- For explicit multi-agent reviews, always include an `Agents:` line naming `Codex`, `Claude`, and `Gemini`. If any did not return, label them `timed out` or `unavailable`.
- **ALWAYS use actual Unicode emoji characters**, NEVER use GitHub-style shortcodes like `:heavy_check_mark:`, `:red_circle:`, etc. Those render as literal text in the terminal.
- Use these exact characters: ✅ ❌ ➖ 🔴 🟠 🟡 🟢 ⚡ 🔒 🏗️ 🧪 📖 🔧 🐹
- For the Agent Consensus table, use ✅ for flagged, ➖ for not flagged
- Keep tables narrow — long cell content breaks CLI table rendering. Use short labels.

Present the unified report using this structure:

```
## 📋 Code Review Council Report

**Files**: `path/to/file.ext` (N files, +X -Y lines)
**Language**: Go 1.25
**Agents**: Codex ⚡ · Claude 🏗️ · Gemini 🧪 · Manual 🔍
**Reviewed Areas**: Security 🔒 · Performance ⚡ · Readability 📖 · Architecture 🏗️ · Testing 🧪 · Maintainability 🔧 · Go Idioms 🐹

---

### Summary
[1-2 sentence overall assessment]
Go Idioms: [finding or `N/A for this language`]

---

### 🔴 Critical

**[Issue Title]** — `file.go:42`
*Flagged by: Codex ⚡, Manual 🔍*

[Description and why it matters]

```suggestion
[Code fix if applicable]
```

---

### 🟠 High

**[Issue Title]** — `file.go:88`
*Flagged by: Manual 🔍*

[Description and recommendation]

---

### 🟡 Medium

**[Issue Title]** — `file.go:120`
*Flagged by: Gemini 🧪, Manual 🔍*

[Description and recommendation]

---

### 🟢 Low

- **[Suggestion]** (`file.go:55`) — [Brief description]
- **[Suggestion]** (`file.go:90`) — [Brief description]

---

### ✅ Strengths

- [Good practice observed]
- [Another strength]

---

### 🤝 Agent Consensus

| Finding                    | ⚡ Codex | 🏗️ Claude | 🧪 Gemini | 🔍 Manual | Confidence |
|----------------------------|---------|----------|---------|---------|------------|
| Hot-path allocation        | ➖      | ➖       | ➖      | ✅      | Med        |
| Missing counter            | ➖      | ➖       | ✅      | ✅      | High       |
| Global state mutation      | ✅      | ➖       | ➖      | ✅      | Med        |

*Legend: ✅ = flagged · ➖ = not flagged*

---

*Reviewed Areas: Security 🔒 · Performance ⚡ · Readability 📖 · Architecture 🏗️ · Testing 🧪 · Maintainability 🔧 · Go Idioms 🐹*
*Agents: Codex (gpt-5.4) · Claude (opus) · Gemini (gemini-3.1-pro-preview)*
```

### Agent Icon Reference

Use these consistently throughout the report to identify sources:

| Agent/Source | Icon | Usage |
|-------------|------|-------|
| Codex | ⚡ | `Codex ⚡` or just `⚡` in tables |
| Claude | 🏗️ | `Claude 🏗️` or just `🏗️` in tables |
| Gemini | 🧪 | `Gemini 🧪` or just `🧪` in tables |
| Manual analysis | 🔍 | `Manual 🔍` or just `🔍` in tables |

### Persona Icon Reference

| Persona | Icon |
|---------|------|
| Security | 🔒 |
| Performance | ⚡ |
| Readability | 📖 |
| Architecture | 🏗️ |
| Testing | 🧪 |
| Maintainability | 🔧 |
| Go Idioms | 🐹 |

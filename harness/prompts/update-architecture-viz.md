# Architecture-viz prompt: keep `harness/architecture-viz.html` true to the repository

Run this after any change to `ARCHITECTURE.md`, `src/README.md`, `src/bin/README.md`, `src/api/README.md`,
`launcher/README.md`, `scripts/harness/check_structure.py`, or the route/matrix definitions, or as part of a
recurring doc-gardening pass ([doc-gardening.md](doc-gardening.md)).

---

You are maintaining [`harness/architecture-viz.html`](../architecture-viz.html), the standalone interactive HTML
visualization of Vesper's high-level architecture, request dispatch pipeline, layer boundaries, and component index.
Your goal is to keep every diagram node, data-flow edge, simulated route, layer rule, and component description in
`harness/architecture-viz.html` 100% faithful to the repository's current implementation and authoritative docs.

## 1. Sources of truth to read first

Before editing `harness/architecture-viz.html`, inspect changes since the `VERIFIED: YYYY-MM-DD` stamp in the header of
`harness/architecture-viz.html` across these files:

1. `ARCHITECTURE.md`:
   - `## System context` (Mermaid flowchart, verified vs. `Intended:` edges)
   - `## Components` (binaries, entry points, owned data, source modules)
   - `## Domain boundaries and dependency rules` (Layers 0–5 table and dependency flowchart)
   - `## Main flows` (`API request` sequence diagram, `VmState` transitions, `validate_state_guard` rules, `Proxy connection`)
   - `## Governance tooling` (artefact paths, purposes, readers)
   - `## Invariants` and `## Known limits` (`TD-nnn` references)
2. `src/README.md`, `src/api/README.md`, `src/bin/README.md`, `launcher/README.md`:
   - Public module list, extracted `src/api/*` route families, binary CLI flags, side effects, and invariants.
3. `scripts/harness/check_structure.py`:
   - `RULES` allowlist and line-budget constants enforced in CI.
4. `docs/specs/design-v0.baselines.lock.yaml`:
   - Pinned `baselines.firecracker.revision` used in simulated `ErrorEnvelope` responses.

Use `git log --since=<last-verified-date> -- ARCHITECTURE.md src/ launcher/ scripts/harness/ docs/specs/` to locate
exact diffs.

## 2. What to update in `harness/architecture-viz.html`

`harness/architecture-viz.html` is a single self-contained HTML file (no external JS/CSS build step) organized into
four interactive views and three JavaScript data structures:

### View 01 — System Context & Data Flows (`#view-context` + `COMPONENTS`)
- **SVG Topology (`svg.arch-svg`)**:
  - Zone A (`Clients & CLI`), Zone B (`macOS Host Daemons & Policy`), Zone C (`System VM Tooling & Swift Helper`),
    Zone D (`Specs, Matrices & VM`).
  - Every node `<g class="arch-node" data-id="..." data-flows="...">` must have a matching entry in the `COMPONENTS`
    object in `<script>`.
  - Every `<path class="arch-edge ...">` must carry accurate `data-flows` (`control`, `proxy`, `governance`,
    `systemvm`, `intended`) and use `.intended` (dashed stroke) for design-only edges and solid strokes for verified
    edges.
- **`COMPONENTS` map**:
  - Update `name`, `category`, `status` (`VERIFIED` vs. `INTENDED`), `path`, `owns`, `invariant`, and `ste` (the array
    of descriptive sentences) whenever a binary, module, artifact path, or `TD-nnn` limit changes.

### View 02 — API Dispatch & State Simulator (`#view-sim` + `SIM_ROUTES`)
- Verify the VM states (`PreBoot`, `Running`, `Paused`) and route-guard behavior in `src/api.rs` (`handle_all` and
  `validate_state_guard`).
- Ensure `PUT /actions` and `PATCH /vm` reflect early guard bypass and handler-specific `VmState` checks, while other
  routes run `validate_state_guard` using `RouteSemantics` (`PreBoot`, `PostBoot`, `Any`, `PreBootRuntime`) from
  `docs/generated/firecracker-api-surface-stub-matrix.json`.
- Keep `baseline_firecracker_revision` in simulated `ErrorEnvelope` payloads synced with
  `docs/specs/design-v0.baselines.lock.yaml`.

### View 03 — Layer Boundaries L0–L5 (`#view-layers` + `LAYER_DEPS`)
- Sync the layer stack HTML (`#layer-stack`) and `LAYER_DEPS` in `<script>` with the `Domain boundaries and dependency
  rules` table in `ARCHITECTURE.md` and `RULES` in `scripts/harness/check_structure.py`.
- For each module key, `allowed` must list only the module keys that the layer table permits it to reference.

### View 04 — Component Index (`#view-ste100`)
- Automatically rendered from `COMPONENTS` by `renderSteTable()`. Confirm that new or renamed components appear and are
  searchable.

### Header Stamp
- Update `<span class="stamp"><span class="dot dot-emerald"></span>VERIFIED: YYYY-MM-DD</span>` to today's date once
  all claims have been verified against the codebase.

## 3. Visual & Writing Constraints (Non-Negotiable)

1. ** Bauhaus Constructivist Aesthetic & 100% Monospace**:
   - Preserve the CSS custom properties (`--paper`, `--ink`, `--red`, `--blue`, `--yellow`, `--border-heavy`,
     `--shadow-hard`) and the global `* { font-family: var(--font-mono) !important; }` rule.
   - Do not introduce proportional fonts, rounded cards (`rx` must remain `0` on node boxes), soft drop shadows, or
     external JS dependencies.
2. **Writing Style (Simplified Technical English as a Silent Guide)**:
   - Write all component, simulator, and layer descriptions (`ste` arrays) using active voice, approved technical
     vocabulary, one topic per sentence, no noun clusters longer than 3 words, and a maximum of 25 words per sentence.
   - **Do not** display "ASD-STE100", rulebook banners, or sentence word-count badges in the UI—use it strictly as an
     internal writing standard.
3. **Evidence Discipline**:
   - Never present an `Intended` or `Proposed` edge/capability (such as Backend A guest containerd/Firecracker or the
     unimplemented `TD-004` host vsock bridge) as `Verified`.

## 4. Verification

After updating `harness/architecture-viz.html` (and any owning markdown docs):

```sh
python3 harness/check_docs.py --strict
python3 -m unittest discover -s harness/tests
python3 scripts/harness/check_structure.py --root .
```

# Maintain `harness/architecture-viz.html` against the repository

Run this after a change to `ARCHITECTURE.md`, a directory contract (`src/README.md`, `src/bin/README.md`,
`launcher/README.md`, `shim/README.md`, `systemvm/overlay/README.md`), `scripts/harness/check_structure.py` or the
route and matrix definitions, or as part of a doc-gardening pass ([doc-gardening.md](doc-gardening.md)).

Update this offline architecture explanation after changes to source contracts,
dependency rules, routes, pipelines or known limits. Keep every factual claim
traceable to current code, tests or an explicitly labelled design document.

## Read the evidence first

Inspect changes since the last verified `data-source-date` in the document.
Start with the repository's architecture notes, module README contracts,
specifications, lockfiles and mechanical structure checks. Read the affected
implementation and tests, not only their summaries.

Retain the difference between observed behaviour, intended design, open
acceptance work and unknowns. Do not promote an illustrative or proposed claim
to Verified because the interface has been restyled. Update the human date and
source revision only after checking the affected claims. Use a date such as
**6 October 2026**, and show the revision once.

## Vesper sources and conventions

Read these before you change a claim. `git log --since=<data-source-date> -- <path>` shows what changed.

| Claim in the page | Source |
| --- | --- |
| Components, edges, invariants, known limits | `ARCHITECTURE.md`, then the code it names. Where the two disagree, the code is right; record the difference in `docs/exec-plans/tech-debt-tracker.md` |
| Rust binaries and modules | `src/README.md`, `src/bin/README.md`, `src/api/README.md` |
| Launcher and bridge | `launcher/README.md`, `src/systemvm/bridge.rs` (`bridge_allowed_ports`) |
| System VM services, agent, jailer, data disk, shares | `systemvm/overlay/README.md` |
| Shim, share server, authorisation plugin | `shim/README.md`, `docs/SECURITY.md` |
| Egress policy | `docs/design-docs/adr/0008-vesperd-microvm-egress.md` |
| Status of each component | `docs/exec-plans/tech-debt-tracker.md` and the suite named in the component's text |
| Layer view (`LAYER_DEPS.allowed`) | `RULES` in `scripts/harness/check_structure.py` and the layer table in `ARCHITECTURE.md` |
| Route count, baseline revision | `testdata/contracts/firecracker-routes.golden.json`, `docs/specs/design-v0.baselines.lock.yaml` |

Status words: `Verified` only when the repository names a test or suite for the claim; `Open · <what is open>` with
the tracker row; `Planned · <state>` for design without code. The page repeats the repository's evidence. Say so in
the provenance paragraph, and say which suites you ran yourself, if any.

Diagram grid (view box 1750 by 1120): node columns start at x = 60, 390, 720, 1080 and 1440; rows at y = 80, 250,
420 and 590 for the runtime band and y = 822 and 992 for the contracts band. Edges between the two bands run in the
gap at y = 716 and y = 738. The lane at x = 30 is outside the zones. One crossing exists (`e-store` with `e-export`);
do not add another without a reason. A dashed arrow means planned work, and a dashed node border means an open item.

Simulator answers are captured, not written by hand. To refresh them, start the model backend on a scratch socket
and replay the requests in each state, then copy the status and the body into `SIM_ROUTES`:

```sh
cargo build --bin vesperd
./target/debug/vesperd --socket run/viz/v.sock &
curl -s -w '\nHTTP %{http_code}\n' --unix-socket run/viz/v.sock -X PUT http://localhost/boot-source \
  -H 'Content-Type: application/json' -d '{"kernel_image_path":"<an existing file>"}'
```

States: `PreBoot` (fresh), `Running` (after `PUT /actions` with `InstanceStart`), `Paused` (after `PATCH /vm` with
`Paused`). Requests: `PUT /boot-source`, `PUT /actions`, `PATCH /vm` (`Paused`, `Resumed`), `PUT /snapshot/create`,
`GET /mmds`, `GET /non-existent-route`. A `ste` array must hold string literals only; the check refuses a
concatenation.

## Preserve the four-view contracts

`harness/architecture-viz.html` is a self-contained HTML file with these stable interfaces:

| View | IDs and source | Required behaviour |
| --- | --- | --- |
| 01 System context | `#view-context`, `COMPONENTS` | Every SVG `data-id` resolves to a component; edge endpoints, flow tags and direction remain correct. Keep tracing, filters, live counts, Fit/100%, pan and inspector jump chips. |
| 02 Run simulator | `#view-sim`, `SIM_ROUTES` | Preserve state and route evaluation, all five stages, exact request/command text and payloads. Keep Passed/Bypassed/Failed/Skipped labels and working Copy. |
| 03 Package layers (L0–L5) | `#view-layers`, `LAYER_DEPS.allowed` | Keep the dependency contract authoritative. Derive Imports and Imported by counts and jump targets from it. |
| 04 Component index | `#view-ste100`, `COMPONENTS` | Retain live search, `/`, Clear, N of M counts, status filters, text-node highlights and links that focus the matching diagram node. |

Update component `name`, `category`, `status`, `path`, `owns`, `invariant` and
`ste` fields when their source changes. Preserve identifiers and exact code,
flags, commands, paths, payloads and error names. Never localise a source token.
Use sentence-case presentation labels without discarding the underlying evidence.

Preserve solid/dashed edge meanings from the local diagram. A dashed edge does
not universally mean the same thing in every project; retain its explicit legend.

## Follow Signal Nord

Read the resolved skill's `references/meister-continuum.md` and
`references/style-and-prose.md`. Change shared values in the canonical Signal
source, regenerate the adapter, then refresh `MC_TOKENS`. Do not change a shared
colour inside a local selector or create another independent palette.

- Keep `system`, `continuum-light` and `continuum-dark` appearance IDs. Resolve
  System once and preserve explicit Light/Dark choices. Keep all views and
  `color-scheme` aligned with that result.
- Use Nord Light and Nord Dark surfaces. Frost Blue denotes action, focus,
  Runner/control, incoming flows and reverse dependencies. Aurora Green denotes
  Deployment/target, verified or passed states, outgoing flows and imports.
  Reserve Aurora Red for guards, errors, failure and invariants.
- Keep surfaces flat and opaque. Use a plain heading icon, quiet metadata,
  borderless text tabs and one provenance paragraph. Avoid rainbow categories,
  coloured top rules, nested boxes, gradients and decorative shadows.
- Use Inter for headings and normal-width IBM Plex Sans for prose, controls and
  diagram labels. Use native monospace for web code. Preserve exact embedded
  WOFF2 fonts and their licences; introduce no font CDN or external dependency.
- Keep PragmataPro Mono Liga under terminal-host control. Do not alter its size,
  ligatures or cell geometry, and do not distribute proprietary font files.
- Use the readable semantic ink aliases on their tested backgrounds. Essential
  boundaries use `borderControl`; quiet hairlines remain decorative. Never fade
  informative labels or complete No import chips to obtain visual hierarchy.
- Preserve Lucide SVGs, explicit status words, visible keyboard focus and
  semantic controls. Keep the inspector unboxed, with a single left boundary
  and section rules.

## Preserve geometry and readable interactions

Use 74-unit node cards, 6-unit corners, 1.1-unit meaningful outlines and clipped
4-unit semantic stripes. Keep badges at y=20, titles at y=41 and supporting text
at y=59. The 100% view renders node titles at 16px; Fit remains reversible.
Do not add an internal divider or opaque header band.

Route edges through text-free orthogonal corridors with 6-unit rounded corners.
Keep arrows clear of zone titles, node labels and other connection labels.
Measure edge labels with `getBBox()` after fonts load and after tab activation.
Their background is transparent at rest; tracing or filtering applies a soft
semantic fill. Retain readable strokes and text when reducing decorative detail.

Trace begins enabled. Incoming relationships use Blue; outgoing relationships
use Green. Support drag-to-pan without triggering selection after a drag. Keep
jump chips usable by keyboard and preserve local scrolling on narrow screens.

Use neutral layer badges and No import chips. Scope strikethrough to the module
name, never its status icon or label. Preserve lowercase CLI syntax inside
uppercase section labels. Render source paths with slash-boundary wrapping.

Highlight search results through text nodes and `<mark>` elements. Do not replace
strings inside `innerHTML` or interpret repository content as HTML. Keep links,
inline technical tokens and empty results intact.

## Write concise British English

Use human headings: System context, Run simulator, Package layers (L0–L5),
Component index, How it works, Owns, Invariant, What happens and Observed imports.
Use **and**, **email**, **startup** and **run ID** in prose. Preserve exact syntax.

Write active sentences with one topic and at most 25 words in every `ste` entry.
Avoid repeated boilerplate and noun clusters longer than three words. Keep these
writing rules silent in the product; do not display guide names or word counts.

## Verify the actual output

Run:

```sh
python3 <resolved-skill-dir>/scripts/render_viz.py --check harness/architecture-viz.html
```

`<resolved-skill-dir>` is the directory of the `explainer-viz` skill. Then run the repository's own checks:

```sh
python3 harness/check_docs.py --strict
python3 -m unittest discover -s harness/tests
python3 scripts/harness/check_structure.py --root .
```

When the actual downstream checkout is available, run its visual-system sync and
full checks too. Do not claim those passed from a separate candidate workspace.

Inspect all four views in both modes at desktop and narrow widths after fonts
load. Initialise capture appearance directly to avoid transition races. Exercise
Tab, Enter/Space, arrow tabs, Home/End, `/`, Clear, filters, trace, pan, jump links,
route/state controls and Copy. Measure informative text at 4.5:1 and essential
graphical boundaries at 3:1 against their actual backgrounds. Inspect routing
collisions, status wrapping, lowercase flags and source paths.

Report source changes, checks performed, actual host/appearance/font and any
untested interaction, export or platform behaviour. A static check or screenshot
does not establish complete accessibility or physical display quality.

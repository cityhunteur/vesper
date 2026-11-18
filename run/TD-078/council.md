# Council on TD-078 (the server is told of a file before the file is attached), 2026-11-18

Working paper (D-007). Target: the change on `fix/td078-live-protect-list`, base `main` at `252830a`.
All contexts ran on the session's own model: the skill's default reviewer and judge models were not available
here, so the reviewers and the judge are separate contexts of one model, not independent models.

## Design assessment, before the code was final

One context (architecture and security). Verdict: proceed with changes. D-1: tell the number of the file that is
in fact attached. D-2: keep the server's looks. D-3: the list's mode must not come from the umask. D-4: read only
what was added and parse strictly. D-5: run the escape checks with a list attached. D-6: list what is at every
place at the start. D-7, D-8: test controls, clean-up, growth. All were taken into the code but one control (a
build with the telling switched off): no switch that turns a security step off was added.

## Review of the implementation

Four contexts, one role each, static only, on the same captured diff (sha256 `89074143…`): security,
correctness, architecture, Go practices. None found a defect that breaks the protection or normal use.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| SEC-1 | low | When the list could not be told, the new file was not attached and not nested, so it stayed writable through the outer bind for as long as the telling failed; SECURITY.md said the binds would refuse changes | The supervisor ends the server. The next start opens the sources by path and writes the list whole, or does not serve. Not run. |
| SEC-2, COR-6, ARC-3 | low | The supervisor never told an object twice, but the server forgets after 768 newer ones | The line is added at every attach. |
| SEC-3, ARC-4, GO-6 | low | The claim was stated without the 768 limit; `run` without `--protect` had no list and told nothing | The limit is stated beside the claim, in D-056 and in ADR 0009. Every sandbox has a list; the branch that wrote a fixed list is gone. |
| SEC-4 | low | The list grows without a limit of its own | After 1 MiB added the supervisor ends the server, which starts with a list written anew. The rate is stated. Not run. |
| SEC-5, COR-1 | low | A short write did not break the list until the next line joined the part, and nothing mended it; two texts said otherwise | The write goes through the file, which finishes a short write; any error ends the server (SEC-1). The texts say what happens. |
| SEC-6, GO-5 | low | The escape checks' list went through a guessable name in `/tmp` | It is in a directory made for it, removed afterwards. |
| COR-2, GO-2 | low | The new check printed `<nil>`, spent the source's budget and could fail for good | Two error texts with the two identities; no budget is spent, so the nests are still placed. The line is added only after the check passed, so a check that fails adds nothing. |
| COR-3 | low | Documents the change made stale (the sandbox's contents, "root only", a misplaced test comment, 43 against 44) | Corrected. |
| COR-4, GO-1 | low | The line was written through a bare descriptor number that `Run` can close | Written through the file; a closed list is recognised and passed over. |
| COR-5 | low | Three places where the parser stopped or dropped without breaking the list (a zero byte, a 65th place, a 257th number) | Each breaks the list. |
| ARC-1 | medium | No one place stated the design as built after three decisions | ADR 0009, "What is bound read-only, as built": the rule, the layers, the stores with their limits and owners, the invariants. |
| ARC-2 | medium | The list's format had four prose definitions and no test on the server's side | The comment in `Config/exports.y` is the definition and the others point to it. New step `broken_list_refuses_every_change`. |
| ARC-5 | low | The rule "the source has a place" was written three times | One helper, `hasPlace`. |
| GO-3, GO-4 | low | A test comment on the wrong test; the mode fix and the no-follow open had no test | Moved; both tested. |

## Judge, on the final change

A fresh context. It checked the patch against the source trees, the digests against their files, and the notes'
numbers against the report files; it confirmed every finding above as fixed or as a stated limit, none rejected,
and found the claim true by construction within the stated limits. Verdict: merge after items 1 to 4.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| J-3 | low | Two source files were edited (comments) after the tested binary was built | Item 1. The shim was built again from the final source and the three suites were run again. |
| J-1 | low | The step stops the whole server; it does not hold one request unfinished, and would pass if the server stopped reading its list inside a request's own check | Item 2. Stated in the TD-078 row; TD-079 (1). |
| J-2 | low | An object told again keeps its old age in the server's ring | Item 3. Stated in SECURITY.md; TD-079 (2). |
| J-4, J-6 | low | One more stale place in SECURITY.md; small errors of fact (when "it attaches nothing" applies, the backup was removed and not empty, two items missing from "Not covered", who else removes a hold) | Item 4. Corrected. |
| J-5 | low | An end the supervisor made for the list was logged as the server's own | It has its own wording in the exit line. |
| J-7 | low | The definition of the list left out three rules the code has | Added to the comment; the server builds to the same digest. |

Checks after the changes: `go vet` (darwin and linux) and `go test ./...` in `shim/`; the server built twice
with the same digest (`d132fd69…`); unfs suite 13 of 13, microVM binds suite 19 of 19, microVM shares suite 12
of 12; `python3 harness/check_docs.py` 0 errors.

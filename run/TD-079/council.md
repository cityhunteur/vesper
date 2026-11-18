# Council on TD-079 (a held request; an object told again), 2026-11-18

Working paper (D-007). Target: the change on `fix/td079-ring-age-and-held-request`, base `main` at `69c9384`.
All contexts ran on the session's own model: the skill's default reviewer and judge models were not available
here, so the reviewers and the judge are separate contexts of one model, not independent models.

Roles: the planner chose correctness, Go practices, testing and readability. Security was added, because the
change is about a security limit. Readability was deferred and not reviewed.

Four contexts, one role each, static only, on the same captured diff (sha256 `a749ed4c…`). None found a defect
in the ageing rule or in the held call; all four said the change could merge. Security: the change makes no
attack cheaper.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| COR-2, SEC-1 | low | A list read again from the start replayed every `s` line, which put the ages back to the order of the lines; the documents said "last told or found" with no such limit | The server keeps how far it took the `s` lines of a list file and does not tell old lines again on a reread. |
| TST-1, SEC-2 | medium | A REMOVE is checked in two places, so the held REMOVE stays refused when the look goes from the one check that CREATE and seven other procedures use | A held UNCHECKED CREATE across two more saves, in the same step. Control 2: with that one look removed, the REMOVE was still refused and the CREATE was allowed. |
| TST-2, COR-1, SEC-2 | medium | Nothing showed that the request was held before the first save (a fixed sleep), and a server that refuses everything would pass | The client makes a marker file when the request is held and the saves wait for it; the client says whether the name was there; after each held request it makes and removes a file of its own, which must go through. |
| TST-3 | medium | The re-told step passed with the choice of the dropped object reversed, or with no new object taken when full | Two more saves with the memory full; the newest backup must be refused. No control run for this part. |
| TST-5 | low | The re-told step took "the second told object" by line number and asserted nothing about the list | The step requires exactly three told objects before the command and says so when it fails. |
| TST-4, COR-4, SEC-6 | low | Wording: "past 768 newer ones", "774 told objects", the 768 read as for each place, step counts in `scripts/systemvm/README.md`, no "Not covered" in the TD-079 row | Corrected; the row lists what is not covered. |
| SEC-3 | low | Nothing failed if the supervisor stopped telling an object a second time | Unit test: a second line is added for an object told again. |
| SEC-4 | low | The record's newest 50 replaced files counted from the first recording | A file recorded again is the newest again; unit test. |
| SEC-5 | low | The re-tell command had no bound; the share server's test hooks were not in SECURITY.md | At most 4,096 on each side; the hooks are listed. |
| COR-5, GO-2 | low | A failed held call left part of a request on a connection that could be used again | The connection is closed, and the comment says so; tested. |
| COR-6, GO-6 | low | The unit test could hang instead of fail | Read deadlines on the server side and a bound on the client's wait. |
| GO-1, GO-3, GO-4, GO-5, TST-6 | low | A wrong failure message in the test, no message for wrong flags, the last `stat` error dropped, two leftovers, argument refusals and a failing wait untested | Fixed, and tested where it is Go. |
| COR-3 | low | Nothing tests that the server's own looks make an object newest | Not done; stated in the TD-079 row. |

## Judge, on the final change

A fresh context. It checked the patch and the two control trees against the source trees, the digests against
their files, and the notes against the reports. It confirmed the rows above, with two exceptions: SEC-3 is not
fixed by the unit test (it shows only that a second line is written, not that the supervisor decides to write
it), and the "tested" in COR-5 was not true (the assertion could not fail). Verdict: merge after J-1.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| J-1 | low | The change to the record put a replaced file that a start finds as a source, with the 150 start numbers full, back without its kind; the next merge lost it and its hold (fail-open) | It stays a replaced file; unit case for two merges in a row. |
| J-2 | low | The new paragraph on test hooks left out two hooks and said each acts on one task's server | Corrected: all hooks, and the three that act more widely. |
| J-3 | low | The assertion that a failed held call closes its connection could not fail | It now requires the closed-connection error. |
| J-4 | low | No decision-log line for the change | D-057. |
| J-5 | low | The server kept its place in a list file that had become shorter | The place is reset then too. No writer shortens a list today. |
| J-6 | low | The step's message cited the backup's content as evidence for the held CREATE, which keeps a file's data | The message says the status is the evidence. |
| SEC-3 | low | Nothing fails if the supervisor stops telling an object a second time | Stated in the TD-079 row as not covered. |

Checks after the changes: `go vet` (darwin and linux) and `go test ./...` in `shim/`; the server built twice with
the same digest (see `unfsd-aarch64.sha256`); unfs suite 14 of 14, microVM binds suite 20 of 20, microVM shares suite 12 of 12;
two control builds of the server and the server binary of `main` each fail the step they are the control for;
`python3 harness/check_docs.py` 0 errors.

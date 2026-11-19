# Council on TD-080 (the path of a read-only bind), 2026-11-18 and 2026-11-19

Working paper (D-007). Target: the change on `fix/td080-anchor-the-path-to-a-read-only-bind`, base `main` at
`9945b4a`. All contexts ran on the session's own model: the skill's default reviewer and judge models were not
available here, so the assessor, the reviewers and the judge are separate contexts of one model, not independent
models.

## 1. Design assessment, before the change (2026-11-18)

Question: how to resolve the stated limit "the server remembers the newest 768 files it was told of".

The assessor found that the limit was not about a user's 768 saves. A task with a read-write bind over the
directory of a read-only bind renames the directory aside and puts a decoy at the path; the supervisor then
attaches the decoy, so the task fills the memory itself, and its next launch binds the decoy. That became TD-080.

Four ways were compared:

| Way | What it does | Verdict |
|---|---|---|
| A. Anchors | The server refuses a rename or removal of a directory on the way to a read-only bind | Recommended: the only one that stops the task's attaches |
| B. One limit everywhere | The record, the pins and the server's memory get the same size | Hardens the memory; leaves the path to the task |
| C. A memory for each source | 768 for each read-only source | Same |
| D. Leave it | State the limit | Same |

Conditions of the recommendation, all taken: the rule covers a raw REMOVE and matches the directory only, not what
lies in it; the chains are found by identity, not by the text of the paths; the time the System VM takes to see a
directory that macOS replaced is measured (`notes.txt`, section 2). Advice not taken: to make the older suite
steps' renames from the System VM in place of a test hook (D-058 says why).

## 2. Review of the first version (2026-11-19)

Roles: the planner chose security, correctness, Go practices, testing and readability. Readability was deferred and
not reviewed. Four contexts, one role each, static only, on the same captured diff (sha256 `0421567d…`).
Correctness and Go practices said the change could merge; security and testing said not yet.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| SEC-1 | medium | The directory of a read-only DIRECTORY bind was never an anchor. After macOS replaced it, the task could rename the new directory aside and put its own there, with no time bound; the next start and the next launch bound it. The step never tried it | The chain of a directory source includes the directory now at its path. The step tries both such directories before and after macOS replaced them; the control shows one going through without the rule. The content of the new directory stays writable: TD-081, shown in `notes.txt` section 6 |
| SEC-2, COR-1 | low | Only the changed chain was told again, so the anchors of the other chains were the first that the server dropped after 2,048 | On any change the supervisor tells every current anchor; it tells at most 1,024; the list's growth limit ends the server here too |
| SEC-3, COR-2, GO-1, GO-3 | low | A chain or a root that could not be looked at was passed over without a word; at a start the source was then served with no chain | A start fails; the watcher logs once and keeps, and holds, what it last told. Not done: a watcher that cannot start still only logs (stated; TD-082) |
| TST-1 | medium | The step did not show that the supervisor told the server of a replaced directory | The step waits for the supervisor's log line and for more anchor lines in the list, and requires both |
| TST-3, COR-4 | medium | The first control never sent the raw REMOVE (a mistake in the call); the binds report predated the launcher | A control on the same build with the rule off: `REMOVE ed status=OK`. All suites rerun on the final build; the notes say which run is which |
| TST-4 | medium | No test of the rule after a server restart; no "Not covered" list in the TD-080 row | Part 3 of the step kills the server and tries again; the row has the list |
| TST-2, GO-5, SEC-6 | medium | Nothing that CI runs covered the watcher's walk; three assertions could pass with nothing checked | `TestCheckChains` (a replaced directory, an unchanged chain, a chain that cannot be walked, a change beside it, the growth limit, a closed list, a list that cannot be written) |
| SEC-4, COR-6, TST-5 | low | The test hook acted on the next server of any task, for its whole life, and left no trace | The hook names one task, the server's log says that it started so, the suite takes the order back when it ends, and the step requires anchors in the list |
| COR-3, GO-2 | low | Every task with a read-write directory bind got a watcher | Only when a read-only source could lie in a read-write one; no read of the sandbox's mounts when no source is a file |
| GO-4 | low | Descriptors left open on an early return; the growth limit not applied | Closed on every return; applied |
| SEC-5, COR-5, COR-7, GO-6 | low | Documents stronger than the code: an unqualified "cannot put a decoy", windows not all named, the cost understated (an anchor is not forgotten in a run), "three kinds" of line, stale rows and comments | Corrected in `docs/SECURITY.md`, ADR 0009, `design-v0.md`, D-058, the patch's comment and the Go comments |
| TST-6, SEC-6 | low | No positive control for a directory inside an anchor; no rename onto an empty anchor; the count did not require the try under another letter case; a check that could not fail | All four changed in the step |
| SEC-6 (third part) | low | No step with `.` or `..` as the name | Not done; TD-082. By the judge's reading the server refuses them |

## 3. Judge, on the final change

A separate context ruled on every finding against the final source (diff sha256 `fee0af49…`) and looked for
defects in the rewritten code. Its checks that ran: the patch body equals the difference of the source trees; the
server binary's digest matches its file; the C differs from what the reviewers read by comments only. It traced
every return of the watcher's walk and found no descriptor left open, and found the suite's counts right line by
line. Verdict: no defect of medium or higher severity in the code; not to merge until the documents were corrected
and this file was added.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| J-1 | medium | `design-v0.md`, D-058, the ADR and the new-launch bullet of `docs/SECURITY.md` were still stronger than the code: none carried TD-081, and `design-v0.md` named only one of the windows | All four corrected |
| J-2 | low | The new log line of the test hook began like a line that the launcher takes for a failed start; a slow start could then fail a hooked launch at random | The line has a prefix of its own (`vesper-unfs test:`). The general case, other warnings with the fatal prefix, is TD-082 |
| J-3 | low | A stated cost was wrong: a directory that a task made at a name of a chain stays an anchor across starts; a file at a chain name and more than 1,024 anchors in the watcher were not stated | Corrected and stated |
| J-4 | low | This file was cited and absent; the TD-080 row said "done" before the push | This file. The row goes to `main` only with the merge, which waits for green CI, as for the units before |
| J-5 | low | Three stale sentences (a Go comment, an ADR line, the unfs3 README) | Corrected |
| J-6 | low | The step's count of anchor lines shows that the server was told, not of what | The comment says so; the refused tries on the new directories are the proof |
| J-7 | low | No unit case for a change beside a chain that cannot be walked, or for the growth limit | Both added. More than 1,024 anchors: TD-082 |
| J-8 | low | The walk opens every prefix from the mount's root each second | Not done; TD-082 |

After the judge the shim changed in one log line and the unit test; the image was built again and the three
suites were run again on it (`notes.txt`, section 8). The code was not reviewed again after those two changes.

## 4. Not reviewed, not run

- Readability was deferred.
- No reviewer or judge ran repository code. The suites and the unit tests were run by the author.
- The Linux-only unit tests were run in an Alpine container from a cross-compiled binary; CI is their first run on
  the pull request's own build.
- The supervisor's limit of open files was not measured (TD-082).

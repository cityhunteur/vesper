# Council on TD-081 (a directory that macOS puts at a read-only directory bind's path), 2026-11-19

Working paper (D-007). Target: the change on `fix/td081-replaced-read-only-directory`, base `main` at `401298b`.
All contexts ran on the session's own model: the skill's default reviewer and judge models were not available
here, so the reviewers and the judge are separate contexts of one model, not independent models.

## 1. Design

No separate design assessment was run. The author weighed four ways and recorded them in D-059: tell the server
of the directory as a read-only object (taken); a new kind of line; end the server so that it starts again;
attach the directory again as a replaced file is. The security reviewer was asked whether the decision leaves a
hole (not to place the nest again, not to attach the source again) and found none.

## 2. Review of the first version

Roles: security, correctness, Go practices, testing. Readability was deferred and not reviewed. Four contexts, one
role each, static only, on the same captured diff (sha256 `88f3cb52…`). Correctness, Go practices and testing said
the change could merge; security said not yet, until the documents stated SEC-1. No reviewer found a fault in the
code.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| SEC-1 | medium | The documents bounded the exposure to "about a second" and to what the task wrote then. Before the server has read the line, a task can hard-link a file of the new directory to a name outside and write that file afterwards, also in later launches; no window is needed when the user makes the directory inside the read-write bind and renames it into place; and a task can move another read-write bind's directory into the new one | Stated in `docs/SECURITY.md` (the rule's "what stays open", and the older sentence "a task cannot make such a link itself" corrected), in D-059, the design and the TD-081 row. No change to the code: the server knows a directory by its number, not each file in it, which is the limit already stated for links made on macOS |
| SEC-2 | low | "About a second" was stated with the example `rm -r conf; mkdir conf`, and only `mv; mkdir` had been measured | Measured (`notes.txt` 5): 2 writes after `mv; mkdir`; 5 and 6, under the other letter case only, after `rm -r; mkdir`. Stated |
| SEC-3 | low | While macOS has nothing at the path, a task can put a symbolic link there; the supervisor then tells nothing | Stated. A second log line was not added: the first says that the path cannot be looked at, with the error |
| SEC-4, COR-1, GO-2 | low | The list's growth was stated as one line for each attach; an attach now adds one more for each such directory. Three comments were inexact | Corrected in `docs/SECURITY.md`, ADR 0009 and the Go comments |
| COR-2, SEC-4 | low | "The mount point shows the old directory until the next launch" does not hold across a start of the server | Measured (`notes.txt` 5): the old directory or "No such file or directory" while the server runs, "Stale file handle" after a start. Stated; the behaviour itself is TD-083 (new) |
| GO-1, COR-3, TST-1 | low | Nothing failed when the call that tells the directories again after a file was removed | The step requires 2 or more new told lines, and no anchor line, when the removed file comes back. Not done: a test of the leaves joined to the list of a start (in the row's "Not covered") |
| TST-2 | low | A refusal in the new directory looked the same as one in the old directory | The task prints what it reads at the two names first; `sees: rnew new` is required |
| TST-3, COR-4 | low | The count of told lines shows one telling, not what was told; the first count was never checked to be set | The comment says so; the first count must be set; the records say "told lines" |
| TST-4 | low | No try through the second read-write bind, none on the user's existing file | Both added |
| TST-5 | low | The failure record lost its end at 900 characters | The task's whole output is written beside the report; the failure record is shorter |
| TST-6 | low | Notes against the files beside them | Notes corrected. Not done: the build's version stamp does not tell the builds apart; the notes say which run is which |
| GO-3 | low | The failure of an anchor line was no longer exercised | A unit case with no read-only directory source |

## 3. Judge, on the final change

A separate context ruled on every finding against the final source (diff sha256 `3c322715…`, which it found equal
to the working tree) and looked for faults in what had changed since the reviewers read. It ran nothing. It
confirmed every finding but the duplicates (SEC-4 of COR-1 and COR-2; COR-3 and TST-1 of GO-1), and found each one
closed or closed in part with the rest recorded. It found no fault in the shim's logic; the non-test Go code of
the two versions differs in comments only. It checked that the patch's change is inside the C comment and that its
digest equals the line in `sources.sha256`, that the step cannot pass with the change absent, and the documents'
numbers against the files here. Verdict: merge, after J-1.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| J-1 | low | The first sentences of the new rule in `docs/SECURITY.md` gave `rm -r conf; mkdir conf` as the example and said that the task goes on reading the old directory; the measurement says it reads an error then | The example is `mv conf conf.old; mkdir conf`, and the sentence says "the old one, or an error when that is gone" |
| J-2 | low | "2 writes" after `mv` were 2 rounds of two writes; "binds suite: 2 writes" was 1 in the last run | "2 rounds of writes"; "1 or 2 writes" |
| J-3 | low | The step asked for 2 more told lines when the file came back; a file attached twice with no directory told would give 2 as well | It asks for 4 (the file and the three directories) |
| J-4 | low | The comment of `ProtectList` still said that `seen` is what is at the protected places | Corrected |

After the judge: the four changes above. The supervisor binary built after the comment change is byte for byte
the one the suites ran on (`build-j4.log`; the digest was compared). The binds suite was run again with the
changed condition (`notes.txt` 7). The code was not reviewed again after these changes.

## 4. Not reviewed, not run

- Readability was deferred.
- No reviewer or judge ran repository code. The suites, the measurements and the unit tests were run by the
  author.
- The measurements of the window are one or two runs each, on one machine, on a volume that ignores letter case.
- No reviewer read all of patch 0006's C code again; it did not change but in a comment.
- The judge noted that the build's version stamp does not tell the builds apart; the notes say which run is which.

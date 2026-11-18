# Review of TD-075 (the share server's crash on a task's first connections), 2026-11-18

Working paper (D-007). Target: the uncommitted change on `fix/unfsd-crash-on-connect`, base `9ba08db`.
One reviewer in its own context (correctness, reliability and security together) read the first version. It did
not work from memory of the library: it disassembled the packaged libtirpc 1.3.5 and the built server. Its
verdict: the diagnosis is right, the patch removes every use of freed or resized library memory in the loop, and
the C code needs no change. No separate judge checked the dispositions below; they are the author's.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| R-1 | medium | The new suite step used fixed task names and an unanchored match of the log tag, so an exit line of an earlier run, or of `z10` for `z1`, could fail a good build | Fixed: the names carry a per-run part, and `unfs-log` matches the whole tag. |
| R-2 | medium | The step judged all exit lines of a task together, so one marked stop hid a server that had ended by itself | Fixed: lines are counted one by one; every start must have an end "as told" and none "by itself". |
| R-3 | medium, unverified | The exit line put the mark and the server's last message at the end of up to 1 KiB, where a system logger may cut | Fixed: "by itself" or "as told", the code and the time come first, then the last 80 bytes of output. A line read back is about 210 bytes and whole. |
| R-4 | low | A reload was marked as a stop; the escape checks logged an "end" under the live server's tag; without `--args` the tag was odd | Fixed: only a signal to stop is marked; no line for the escape checks; a plain tag without `--args`. |
| R-5 | low | Twelve starts miss the fault 1.4% of the time at the rate measured on `main`, and more at a lower rate | Kept at twelve. A probe that holds connections open and adds two at once would be sharper; not built. |
| R-6 | low | Five places where docs or comments disagreed with the code | Fixed (the README's patch count, the launcher's usage line, a misplaced sentence, the patch's comment on `svc_run`, "every end"). |
| R-7 | low | A descriptor number closed and taken again in one round makes the loop wait for the new connection's first bytes, as the old loop did; a failed `realloc` retries with no message | The first is in the patch's comment. The second is left: a message would change the reviewed binary for no behaviour. |

Checks: `go vet` (darwin and linux) and `go test ./...` in `shim/`; the server built twice with the same digest,
and again after the comment edits with the same digest; 0 failed starts of 40; microVM shares suite 12 of 12,
unfs suite 13 of 13, microVM binds suite 13 of 13; `python3 harness/check_docs.py` 0 errors.

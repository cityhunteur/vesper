# Review of TD-076 (hold what the record names) and TD-077, 2026-11-18

Working paper (D-007). Target: the change on `fix/td076-hold-recorded-objects`, base `main` at `9932d8c`.
One reviewer in its own context (security, correctness and reliability together), static only, read the first
version: holds as mounts in the System VM's own mount namespace, a look by the server on every turn of its loop
(idle wait 2 s), and a 3 s wait in the supervisor between two attaches. Its verdict: not to merge before H-1 is
fixed and H-2 and H-3 are fixed or stated. A separate judge then read the second version and this table.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| H-1 | high | `cleanup` runs `find /run -type s` as root under the lifecycle lock; with holds mounted under `/run` it would walk every other task's held directories through virtio-fs, fill the kernel's caches, and could report a false `DIRTY … socket` | The design changed: the holds are in a mount namespace of their own, kept by one bind at `unfs/ID.prot.d/ns`. In the System VM's namespace there are only empty mount points. Seen: `vesper-share list` shows the three shares while two objects are held. |
| H-2 | medium | A client can keep the single-threaded server out of its main loop (many requests in a row, or one unfinished record for up to 35 s), so the look on each turn does not happen | The server also looks at every request, so a stream of requests is a stream of looks. One unfinished request still stops the looks for up to 35 s: stated under "Not covered" in SECURITY.md and in the TD-076 row. Not tried. |
| H-3 | medium-low | The 3 s wait delayed the attach of the newest version of a read-only file after two quick saves, which widened the window of D-052 | The wait is 750 ms, below the one second between the supervisor's checks, so it delays nothing; the server's idle wait is half a second instead. Step 6l now makes the second save as soon as the first is attached. |
| H-4 | low | `ReleaseHolds` would unlink any entry of the directory, follow a symbolic link at it, and act on any path given | It acts only on names of the form DEV-INO and the namespace file, refuses a path that is not a directory, removes a directory only with rmdir and a file only when it is a regular file of no length, and in the System VM's namespace unmounts only the bind of the namespace file. |
| H-5 | low | No step failed without the hold of a replaced file across a restart; 6l passed without the wait; holds were not checked to exist during a task; the prune has no step | 6k requires holds before the kill. 6l kills the server and drops the caches after its first tries and tries again (no control run for that half). The prune still has no step; stated in the TD-076 row. |
| H-6 | low | A source served read-only for an error could still be recorded later (a replacement on macOS, or a read-only twin of the same path) | Such sources are passed to the watcher, which neither records nor holds for them, and "bound both ways" is judged by the sources as configured. |
| H-7 | low | A failed hold left a recorded number with nothing behind it; the launcher discards the release's status | The hold is made before the record at an attach. `cleanup` reports `unfs-holds` when the hold directory is still there. A failed hold is still only logged; stated. |
| H-8 | low | Holds appeared wherever every virtio-fs mount is taken as a share (four places) | Gone with H-1's change: the holds are not in that namespace. |

## Judge, on the second version

A fresh context, static only. It checked the patch against the two source trees and the digests against their
files, confirmed H-1, H-3, H-4 and H-8 as fixed and H-5 and H-7 as stated limits, and found no path by which the
release can reach user data. Verdict: merge after items 1 to 4.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| K-1 | medium | The hold namespace was a copy of the System VM's whole mount tree, which kept every share, the data disk and other tasks' mounts alive for the task's life, while three documents said only the holds were in it | Item 1. The namespace gets a small tmpfs as its root and the copied mounts are dropped; the mount points are inside it. Suites run again: 18, 13 and 12. Seen: another task stopped, cleaned and started again while a hold existed. |
| K-2 | medium | The stated limit said a task can stall its server for "up to 35 s"; that is for each read, and a byte now and then keeps it going | Item 2. Restated without the bound in SECURITY.md and the TD-076 row; TD-078 opened, with the lasting fix. |
| K-3 | low to medium | The task in step 6l looked up a file twice a second, so it was not idle and the server's own look had no test | Item 3. The task sleeps. The guest's own requests after each attach are looks too, so the look on each loop turn still has no test of its own; stated in the TD-076 row and the notes. |
| K-4 | low | The watcher recorded and held by another rule than the start (one file given both read-only and read-write) | One rule, computed at the start and passed to the watcher (`Config.unrecorded`). |
| K-5 | low | The bind of the namespace file needs `/run` not to be a shared mount | Stated in SECURITY.md. |
| K-6 | low | The idle cost of two wake-ups a second is not measured | Stated in the notes as not measured. |
| H-7 | low | A comment said no number is recorded for a file that nothing holds; it is, when the hold fails | Item 4. The comment says what happens. |

Checks: `go vet` (darwin and linux) and `go test ./...` in `shim/`; the server built twice with the same digest
(`52e43cef…`); microVM binds suite 18 of 18, unfs suite 13 of 13, microVM shares suite 12 of 12; control on the
build of `main`: both new steps fail; `python3 harness/check_docs.py` 0 errors.

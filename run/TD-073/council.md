# Review of TD-073 (what is bound read-only stays read-only under every name), 2026-11-18

Working paper (D-007). Target: the change on `fix/td073-read-only-by-identity`, base `main` at `71d3c05`.
Two rounds. Each round had a security reviewer and a correctness and reliability reviewer, each in its own
context, static only. A separate judge then read the second round's findings and what was done about them.

## Round 1, on the first version (a walk of the literal path, numbers in a table)

| ID | Severity | Finding | What was done |
|---|---|---|---|
| SEC-1, COR-2 | high | A symbolic link in the path defeated the walk: unfs3 follows links, the walk did not | The directory part is resolved with `realpath`; a last-name link is followed for the calls that act on its target. Shown by `touch -h` through a link (control: the time changed). |
| SEC-2, COR-1 | high | LINK did not check its source, so a file could be linked out of a read-only directory | LINK checks its source. Shown by two `ln` tries (control: one went through). |
| SEC-3 | medium | After a restart, a read-write source rooted under a recorded directory was writable | `GuardSources` at a start; round 2 found the same at a re-attach (S-3). |
| SEC-4, COR-4 | medium | The record dropped the first numbers first, and a task could drive that | The first 150 start numbers stay; only replaced files are capped (newest 50). Unit test. |
| SEC-5 | unverified | The walk trusts cached inode numbers | Tried once, not seen (notes 5). Stated as not ruled out. |
| SEC-6, COR-8 | medium | Documents claimed more than the code did | Rewritten after each round. |
| SEC-7, COR-7 | low | The reload freed the list inside a signal handler | The handler sets a flag only. |
| SEC-8, COR-3 | medium | The list loaded lazily and failed open | A list that cannot be read refuses every change, logs once, and is tried again each second. It still loads at the first changing request. |
| COR-5, COR-6 | medium, low | The suite step could pass with the walk broken, and failed on a volume that keeps case | More tries, a moved read-only directory, pids before and after the kill, fixture links on a volume that keeps case. |

## Between the rounds, found by the author

- A file that replaced a read-only file lost its protection when the task moved it aside (notes 7). The server now
  remembers every object it found at a protected place.
- The System VM gives a forgotten file another inode number (notes 8). The supervisor holds replaced files open.
  The restart case is right only until the System VM forgets an object the task moved away: TD-076.

## Round 2, on the second version

| ID | Severity | Finding | What was done |
|---|---|---|---|
| S-1 | medium | The places were looked at once a second at most, and a task could keep a replaced file from ever being seen | The server looks before every changing request. Cost measured with eight read-only binds: none seen (notes 4). |
| S-2, C-4 | medium | Some tests passed with their check removed; reasons and the guard's log line were not asserted | Every refusal must be "Read-only file system"; the guard's line is required; new tries (rename onto the file, `touch -h`, a hard link from a symbolic link, mkdir and symlink after the move). Control run with the server of `main`: 14 and 3 changes went through (notes 9). The raw-client link probes still pass on that server; stated in the tracker. |
| S-3 | low | The re-attach after a restart did not compare a read-write file source with the record | It does, and attaches read-only on a match. New step `reattached_under_a_recorded_object_is_read_only`; no control run. |
| S-4 | low | A new launch starts with an empty record; SECURITY.md did not say so | Stated under "Not covered", with the reason. |
| S-5 | low, unverified | LINK took a symbolic link source as the link itself; a file system may link the target | LINK follows the source's last name in the check. The try `ln /d/L3 /d/k3` is refused (control: it went through). |
| S-6, C-2 | low | Errors read as "not protected": the last name's `lstat`, a walk error in `GuardSources`, a failed `strdup` | Only "not there" is read so; any other error refuses. A walk error in the guard counts as a match. A read error or failed allocation marks the list broken. A place whose `lstat` fails is still skipped for that request. |
| S-7 | unverified | Old numbers from the System VM's cache | Not ruled out; stated in SECURITY.md. |
| S-8, C-1 | medium | A link to nowhere in a read-write bind answered ROFS for its own attributes, and for a create through it, while any read-only bind existed | A followed link is judged by its own text. A target that is not there is left to the call. Probes `link-nowhere` and `link-new`, and `chown -h` in the suite. |
| S-9, C-7 | low | Record details: a replaced file's number kept its kind; the file was written when nothing changed; a temporary file was left | A number a start sees becomes a start number; no write when nothing is new; the launcher removes the temporary file. The number is still recorded after the swap, and a failed write is logged only. |
| C-3 | low | One failed open of the list refused every change for the server's life | Tried again each second. |
| C-5 | unknown | Does the device give a used number to another file | Measured: not in 300 tries; and a forgotten file gets a new number (notes 8). |
| C-6 | low | Sentences the code did not match (`EBUSY`, "every second", a dead constant, the usage text) | Corrected; the link limit is now used (40). |
| C-8 | low | The same object bound read-only under one spelling and read-write under another is read-only through both | Kept, and stated in SECURITY.md. |

## Judge, on the final change

A fresh context, static only. It checked that the patch equals the difference of the two source trees, and that
the patch and binary digests match their files. It confirmed each round-2 finding as fixed or as a stated limit,
and found no link or chain of links for which the server's check allows a change to a protected object. Verdict:
merge after items 1 to 3 below.

| ID | Severity | Finding | What was done |
|---|---|---|---|
| J-1 | medium | The documents said every listed object is held while a server runs. An object the task moved away before a restart is named only by a recorded number, and nothing holds it for the rest of that server's life. A task may force the forget with lookups charged to the server's 32 MiB cgroup (not measured) | Item 1. Wording corrected in SECURITY.md, the tracker (TD-076, now P0, with the cgroup named), the decision log, the notes and `protect.go`. TD-076 is the next unit. |
| J-2 | low to medium | `docs/specs/design-v0.md` stated the feature with no limit | Item 2. The three limits are named there, with a pointer to SECURITY.md. |
| J-3 | low to medium | No test fails without the recorded numbers in the server, or without the supervisor's hold on a replaced file; one step waits 2 s for the guest | Item 3. Recorded as TD-077. |
| J-4 | low | A transient error in the restart comparison makes a bind read-only for the rest of the launch | Recorded as TD-077. |
| J-5 | low | The exemption of a read-write bind inside a read-only one goes by path, not by object | Stated under "Not covered" in SECURITY.md; TD-077. |
| J-6 | low | The rule for a target that is not there rests on the kernel refusing a create on a name with a trailing slash, and on musl resolving paths without `/proc` | TD-077 (strip trailing slashes). The musl in the System VM is 1.2.5. |
| J-7 | low | A link that points at a protected object cannot have its own owner or times changed | Stated in SECURITY.md. |
| J-8 | low | The file a source had at start is held by the start's descriptors, not by the hold list; nothing says so | TD-077. |

## Checks

`go vet` (darwin and linux) and `go test ./...` in `shim/`; the server built twice with the same digest
(`243795b6…`); microVM binds suite 16 of 16, unfs suite 13 of 13, microVM shares suite 12 of 12;
`python3 harness/check_docs.py` 0 errors.

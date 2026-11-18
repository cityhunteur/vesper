# Council review of D-052 (a bound file that macOS replaces is bound again), 2026-11-18

Working paper (D-007). Target: the uncommitted change on `fix/file-bind-replaced`, base `fc41ec7`.
Three reviewers, each in its own context, read the first version: security, correctness, reliability. None found a
critical or high defect. The maintainer found one while checking the security review's reasoning (MNT-1 below),
and the second version was built around it. A separate judge then read the second version (last section).

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| MNT-1 | high | Not found by a reviewer. The supervisor took a replacing file from the share mount. A task with a read-write directory bind can rename directories until the path of its read-write file bind names a file that another bind made read-only; the supervisor then attached that file read-write | Fixed: when a read-write directory source holds the path (`Holders`), the file is taken through that source's export inside the sandbox, where the nest covers it and the clone stays read-only. Suite step `renamed_into_place_keeps_read_only`; negative control in `notes.txt` 6a (macOS read `protected evil` without the fix). |
| SEC-1 | medium | In the window of a nested read-only file, a task can keep write access: rename the new file away, link it, or hold it open | Recorded, not closed (SECURITY.md, TD-074). The docs no longer say the window just ends after a second. A fix needs a write refusal in `unfsd` keyed on the path. |
| SEC-2 | medium | The notice ran in the same loop as the swap, and a guest could stall it | Fixed: notices go from a loop of their own, with at most 30 tries after a swap. |
| SEC-3, REL-3 (nest part), COR-5 | low to medium | A nest was placed again only when the file's identity changed, and state was kept per source | Fixed: the supervisor reads the sandbox's mounts each second and places any nest of a file source that is gone; a place already on a read-only mount is left alone, so nothing is stacked. Directory nests are still not placed again (TD-074). |
| SEC-4 | low | The sandbox was named by process number at each use, and the watcher started before the sandbox was complete | Fixed: the process directory and the mount namespace are held from the start, and nothing is done until the first process has become the server. |
| SEC-5, COR-6 | low | Docs and comments disagreed with the code | Fixed in SECURITY.md, ADR 0009, design-v0 and the comments named. |
| SEC-6 | low | A task could make the server add mounts without bound | Fixed: 20 attaches or nests at a stretch for one source, then one in 15 s. |
| COR-1 | medium | A file bound both read-write and read-only had one hidden mount in the guest; the read-write bind was compared with the read-only mount and bound again read-only | Fixed: one hidden mount for each export and mode. The suite step binds one file both ways and then writes through the read-write bind. |
| COR-2, REL-8 | medium | The suite step could pass with the server's nest never placed; it measured samples, not seconds; it could leave a task | Fixed: the step removes the guest's own bind and writes through the outer export, then checks macOS; samples carry the guest's uptime; a task that does not stop is killed. |
| COR-3 | low | The guest's re-bind did not follow a symbolic link at the end of the destination, as the first bind does | Fixed (`resolveDest`). No runtime test: the test image has no such link. |
| COR-4, REL-1 | medium | The sandbox dies with the thread that started it, and attaching a file ends threads on purpose | Fixed: `Run` keeps its thread (`runtime.LockOSThread`). |
| REL-2 | medium | The guest's convergence was assumed, not confirmed | Fixed: the guest answers each notice (`BIND N same`, `again`, `failed`, `none`); notices stop after two `same` in a row, and a guest that never confirms is logged. |
| REL-3 (dead file) | medium | The supervisor might attach the old, dead file while recording the new identity | Fixed in part: a new file must read before it is attached, and the attached file is read at the next three checks; a file that does not read is attached again. Not shown to happen. |
| REL-4 | low | One loop did every wait | Fixed: one try for each file at each check; the notices have their own loop. A hung virtio-fs request still stops the checks of that server (TD-074). |
| REL-5 | low | A replay process could wait for ever on a dead server, and nothing capped their number | Fixed: 20 s for one event, then the process ends; at most 16 replay processes. |
| REL-6 | low | After a server restart the file export's tmpfs may get another device number | The common case is a suite step (`replaced_after_a_server_restart`, passes). The case where another mount took the number in between is recorded (TD-074). |
| REL-7 | low | Write loss around a replacement was not stated | Recorded (ADR 0009, design-v0, TD-074). |

Also found while reviewing, older than this change: TD-073 (a read-only bind is not protected when a second
read-write bind lies inside the first; shown in `notes.txt`'s source run and the tracker row).

Checks on the second version: `go vet` (darwin and linux) and `go test ./...` in `shim/`; microVM binds suite 13 of
13, unfs suite 13 of 13, microVM shares suite 11 of 11 on a rebuilt System VM; `python3 harness/check_docs.py` 0
errors.

## Judge

A fresh context that wrote neither the code nor the reviews read the second version. It confirmed the dispositions
above (MNT-1 on the author's run for one point; the record for REL-4 was missing and is added), found no path that
loses the read-only state of a clone, and judged the change fit to merge. Its new findings:

| ID | Severity | Finding | What was done |
|---|---|---|---|
| JDG-1 | medium | A nest whose place lies under another read-only mount was looked at every second, spent the source's budget and was logged as placed | Fixed: `placeNest` says whether it mounted; a covered nest is left for 30 checks, costs no budget and logs nothing. |
| JDG-2 | medium | Paths are compared by their bytes, and a macOS volume ignores letter case: a task may write a nested read-only file under a name in another case | Run and confirmed (`notes.txt` 7b). Older than this change. Recorded with the other ways round a nest as TD-073, P0, and SECURITY.md now says the layout is not a boundary. |
| JDG-3 | low to medium | A server that restarts opens every source by path again, after any renames the task made | Recorded in TD-073 (not run). |
| JDG-4 | low | Another task with a read-write bind above the file can change what the path names | Recorded (SECURITY.md, TD-074). |
| JDG-5 | low | A read-write file bind under a read-only directory bind turns read-only after the first replacement | Recorded (TD-074). |
| JDG-6 | low | Two attaches during one notice could count an old answer | Fixed: each attach has a number, and an answer to an older one is dropped. |
| JDG-7 | low | The 20 s limit also ended the fanout's long-lived replay and lost its queued lines | Fixed: the limit applies to notices only. |
| JDG-8 | low | The suite counted re-bind lines, not destinations, and hid a failed `umount` | Fixed: four distinct destinations are required, and a failed `umount` fails the step. |
| JDG-9 | low | A relative link target was joined to the text of its directory | Fixed: it is joined to the directory's real path. No runtime test. |
| JDG-10 | low | TD-074 lacked the hung request, the 15 s wait with a spent budget and the nests never removed; a symbolic link as `--unfsd` would never be "ready" | Recorded; the server's name is taken from the resolved path, and a sandbox that is not ready after 30 checks is logged. |

# Council review of D-037, D-038, TD-066 and TD-067 (2026-11-17)

Target: main at e03dbda (share-server code unchanged to 635d643). This was a static review.

- Reviewers: security, reliability, architecture and correctness, each a separate agent.
- Judge: a separate agent on Opus.
- The owner asked the council to decide D-037 and D-038, and only to assess TD-066 and TD-067.

## Judge outcomes

| ID | Outcome | Severity | Disposition in this change |
|---|---|---|---|
| SEC-1 | Confirmed | medium-high | Fixed: the `/dev/null` bind is read-only. A selfcheck checks chmod, chown and write, and the probe tries a SETATTR through a symlink. Runtime: the mount is `ro`, the probe passed 67 of 67, and the System VM's `/dev` is devtmpfs. |
| SEC-2 | Confirmed | low | Residual in SECURITY.md. The probe records that `/etc/exports` is readable through a symlink. |
| SEC-3, SEC-5 | Confirmed | low | TD-069 |
| SEC-4 | Confirmed, impact Unknown | low | TD-069 |
| REL-1 | Confirmed | high | Fixed: a SIGKILL to init starts the 10 s fallback even when the guest refuses the signal. There is a unit test that fails without the fix. |
| REL-2 | Confirmed | medium-high | TD-068, P0. Runtime: all 4 rename cases lose the old content. |
| REL-3 | Confirmed (also at runtime: `.369521199` became `.000000000`) | medium | TD-066 raised to P2. The suite pins the floor. A guest-side rounding was tried and reverted, because unfs3 reports whole seconds to the guest too. |
| REL-4 | Partly confirmed | low-medium | TD-070 |
| REL-6 | Uncertain | info | Documented in SECURITY.md |
| COR-1 | Confirmed | low | Fixed in SECURITY.md |
| COR-2 | Confirmed | low | Fixed: TD-066 cites the new full-precision record |
| COR-3 | Confirmed | low | Fixed: depth counted from the export, 40 |
| COR-4 | Confirmed | medium | Fixed: the step pins today's outcome and measures 3 more cases |
| COR-5 | Confirmed | low | Fixed |
| COR-6 | Uncertain | low | TD-065 wording softened |
| COR-7 | Confirmed | low | Fixed: re-queue counters |
| ARC-2, ARC-3 | Confirmed | medium | Restated as Vesper limits: GH #207 and GH #208 |
| ARC-4, ARC-5 | Uncertain | input | Inputs to the owner's TD-068 decision |
| ARC-6 | Confirmed | low | TD-067 split into TD-067 (depth) and TD-068 (data loss) |

## Rulings

- **D-037:** approved with conditions. Conditions met: SEC-1 fixed and tested; SEC-2, SEC-3 and SEC-5 recorded; SEC-4 recorded as Unknown.
- **D-038:** approved in part.
  - REL-1 is fixed.
  - Not accepted: the data loss after a rename (TD-068).
  - The two refusals are restated as Vesper limits.
- **TD-066:** the judge's ruling is to patch unfs3 for nanosecond times. That is a packaging change, so it is the owner's decision.
- **TD-067:** split. Keep the depth limit as P3. For the data loss, spike a replacement server.

## Found while verifying

- **TD-071:** a directory deleted and remade on macOS can still be the old one in the System VM.

# Council review

**Status: complete for the selected static scope.** 4/4 selected roles completed. 4 confirmed defect(s); 1 unverified or disputed claim(s).

**Request:** Review GH \#160 M6 \(ADR 0009 event hop\)\. The System VM share notification sink \(BusyBox ash, vesper\-share notify; input is macOS paths from the trusted host launcher over vsock, peer\-checked\) starts a root vesper\-unfs fanout process through a fifo before dropping to container uid via setpriv, and writes each replayed path to fd 3\. The Go fanout \(shim/internal/shares/fanout\.go\) maps a path to export\-relative events per microVM from /run/vesper/microvm/unfs/&lt;id&gt;\.args, keeps a bounded coalescing queue per microVM, and sends 'N&lt;TAB&gt;rest' lines to the guest's vsock port 1026 via the jail v\.sock CONNECT handshake \(dial\.go\)\. The guest \(vesper\-console shares replay, guest\_linux\.go Replay/touchInRoot\) parses lines, maps exports to mounted destinations from the session config annotation, opens the path beneath /newroot with openat2 RESOLVE\_IN\_ROOT and sets its mtime to its own value via utimensat on /proc/self/fd\. The sink also now waits for /run/vesper/containers before choosing its uid \(a fixed race\)\. Press on: privilege boundaries \(does anything run as root that should not; can a container or a guest reach port 1026 or the fanout\); fd and fifo handling and SIGPIPE; queue bound, coalescing correctness, the sender goroutine lifetime, races \(go test \-race passes\) and leaks when microVMs come and go; the echo argument \(setting mtime to its own value via NFS; stale attributes; the window where a host write lands between open and utimensat\); path mapping edge cases; correctness of the uid\-wait fix; test validity of the suite step\.

**Evidence:** revisions; snapshot `c7b32ada72fd510ce13ba650318c2c4b063fcd06af97a43b3163915923258aef`.

**Base:** `1e2e3700ef3c29016fe43c674d9dbd66b4a18fda`. **Head:** `91be49578da99a80e1d30ec40d88ec55a5143fb7`.

**Verification:** static source assessment only. This pipeline did not execute repository code, tests, reproducers or reviewer-suggested commands. A completed review is not proof that the code is defect-free.

## Coverage

| Role | Runner | Status |
|---|---|---|
| security | codex:default | completed |
| correctness | codex:default | completed |
| reliability | codex:default | completed |
| testing | codex:default | completed |


**Judges:** claude:opus: completed.

Judge independence is based on separate configured CLI providers and fresh sessions; model aliases do not prove distinct underlying models.


**Scope notes:**

- Static evidence only: no repository code, tests or suggested commands were executed\.
- Related context files are evidence, not automatic routing targets\.
- Some negative wording was not mapped to a role exclusion\. The original request remains authoritative; use \-\-exclude for an exact role constraint\.


## Confirmed defects

### Medium · Replay cannot notify watchers on read\-only share mounts

**review-02-f01** · defect · shim/internal/shares/guest\_linux\.go:272

Host edits to a read\-only bind never produce the intended guest notification: replay attempts to set mtime through the read\-only NFS mount, which fails with EROFS even when replay runs as root\.

**Source evidence:** Static evidence: guest\_linux\.go:134\-140 mounts read\-only shares with MS\_RDONLY\. Replay maps every configured export to its destinations at lines 218\-220 and calls touchInRoot at line 247\. Its only notification operation is UtimesNanoAt at line 272, and failure is counted as missing at lines 249\-250\. Fanout includes read\-only sources \(fanout\_test\.go:17\-19 explicitly expects such mapping\)\. The added suite exercises only a read\-write bind \(microvm\-shares\-suite\.sh:85\)\. No checks were run\.

**Suggested change:** Provide a notification mechanism that works for read\-only destinations, or explicitly limit the supported event\-hop contract to writable mounts and expose read\-only replay failures distinctly\.

**judge-01 — confirmed:** Read\-only binds are mounted with MS\_RDONLY\. utimensat on a file under a read\-only mount fails with EROFS at the VFS \(mnt\_want\_write\) whatever the caller's privilege\. Fanout's Server\.Map does not filter on Source\.ReadOnly; the test at fanout\_test\.go:17\-19 expects an event for the ro export\. Replay sends every such event to touchInRoot, whose only operation is UtimesNanoAt, and counts the failure as 'missing'\. The same applies to an rw\-export event whose path crosses into a nested ro mount \(/data/conf inside /data\): it fails with EROFS too\. So host edits under read\-only binds never produce a guest notification\. Nothing in the ADR's M6 description excludes read\-only binds, and the failure is misreported as 'missing'\. Medium is reasonable for a feature gap on a documented bind configuration\. Evidence: guest\_linux\.go:135\-137 \(MS\_RDONLY for s\.ReadOnly\); fanout\.go:69\-93 \(Map ignores ReadOnly\); fanout\_test\.go:17\-19 \(ro source mapped to Export 1\); guest\_linux\.go:246\-251 and 271\-272 \(UtimesNanoAt only; failure counted as missing\); microvm\-shares\-suite\.sh:85 tests an rw bind only\. Static inspection; nothing was run\.

### Medium · Replay can overwrite a newer host modification time

**review-02-f02** · defect · shim/internal/shares/guest\_linux\.go:271

If a host write lands after Fstat obtains the mtime and before UtimesNanoAt applies it, replay writes the older timestamp back to the host file\. This breaks the claimed preservation of host mtime and can create another apparent edit for the launcher's timestamp filter\. Fresh attributes at open do not close this window\.

**Source evidence:** Static evidence: guest\_linux\.go:268 reads st, then lines 271\-272 unconditionally send st\.Mtim back through NFS\. There is no concurrency check or recovery between those operations\. The comment at lines 204\-208 claims that host mtime remains unchanged\. The existing System VM replay explicitly recognises and partially handles the analogous race in vesper\-share:94\-98; the new guest replay has no corresponding handling\. The host watcher implementation is not supplied, so the precise downstream echo behaviour is not established\. No reproducer was run\.

**Suggested change:** Avoid writing a previously sampled timestamp back as the notification primitive, or implement and document a concurrent\-write policy that prevents silently restoring an older host mtime\.

**judge-01 — confirmed:** touchInRoot reads the mtime with Fstat after openat2, then sends that value back in a separate SETATTR through utimensat\. Nothing is conditional or checked between the two calls\. A host write committed on the server between them has its newer mtime replaced by the older sampled value\. That goes through unfsd onto the virtio\-fs share and so onto the macOS file\. The comment at guest\_linux\.go:204\-208 claims host mtime and size stay as they were, which this window contradicts\. The System VM sink explicitly handles the analogous race \(vesper\-share:96\-98\); the guest replay has no equivalent\. The window is narrow, but the consequence is a silently regressed host mtime\. The finding correctly leaves the downstream launcher echo behaviour unestablished\. Evidence: guest\_linux\.go:267\-272 \(Fstat, then UtimesNanoAt with st\.Mtim, no recheck\); guest\_linux\.go:204\-208 \(claim that mtime is preserved\); vesper\-share:94\-98 \(existing size\-change recovery for the same race\)\. The launcher's filter is not in the evidence\. Static only\.

### Medium · Concurrent statistics writers can publish truncated or invalid JSON

**review-02-f04** · defect · shim/internal/shares/fanout\.go:339

The input reader and independent sender goroutines write the same temporary statistics file without serialisation\. One writer can rename that file while another has it open and is truncating or writing it, exposing incomplete or malformed statistics and undermining the suite's queue\-counter comparison\.

**Source evidence:** Static evidence: fanout\.go:310 calls writeStats from each sender, while Run calls it at line 353\. Snapshot locks only while copying counters \(lines 324\-330\)\. All callers subsequently use the identical StatPath\+'\.tmp' at lines 339\-341 with os\.WriteFile and Rename outside that lock\. File operations are therefore concurrent despite the counter map being protected\. microvm\-shares\-suite\.sh:91\-99 relies on parsing and comparing the published JSON\. No race test or filesystem reproducer was run\.

**Suggested change:** Serialise the complete snapshot/write/rename operation with a dedicated mutex, or use unique temporary files and ordered publication that prevents older snapshots from replacing newer ones\.

**judge-01 — confirmed:** Run and every per\-microVM sender goroutine call writeStats\. Snapshot's mutex covers only the copying of counters\. os\.WriteFile \(open with O\_TRUNC, then write\) and os\.Rename on the shared StatPath\+'\.tmp' happen outside any lock\. Interleaving A open → B open \(same inode, truncate\) → A write → A rename → B write leaves the published fanout\.stat inode being truncated and rewritten while readers can see it\. An older snapshot can also replace a newer one\. The suite's q1/q2 extraction with sed reads this file through \`microvm unfs\-events\`, so a transiently empty or stale file can make that step fail or misjudge\. The impact is limited to observability and suite reliability, not delivery; medium is at the upper bound but defensible because the suite gates on it\. Evidence: fanout\.go:306\-310 \(sender calls writeStats\), 349\-353 \(Run calls writeStats\), 324\-331 \(lock only in Snapshot\), 334\-343 \(shared tmp path, WriteFile and Rename unlocked\); microvm\-shares\-suite\.sh:91\-99 \(suite parses and compares the published counters\)\. Static; no race test was run\.

### Low · Fanout delivery test requires scheduler\-dependent coalescing

**review-04-f01** · defect · shim/internal/shares/fanout\_test\.go:107

The test rejects a valid execution where the sender drains each event before the next duplicate is offered\. Coalescing applies only to waiting events, so all 1,001 events may legitimately be sent; the st\.Sent &gt; 1000 assertion would then fail\.

**Source evidence:** fanout\_test\.go:88\-92 feeds duplicates while the sender runs concurrently\. fanout\.go:125\-129 removes taken events from the waiting set, allowing subsequent identical events to queue again\. fanout\_test\.go:107 nevertheless requires at least one event to coalesce\. Static inspection only\.

**Suggested change:** Gate the fake Dial until Run has queued the input, then release delivery and assert deterministic coalescing, or leave coalescing to TestQueueCoalescesAndBounds and assert only delivery/accounting here\.

**judge-01 — confirmed:** Coalescing applies only while an event is still waiting\. Take removes events from the waiting set, so a later identical Offer queues again\. The sender goroutine runs concurrently with Run\. If the scheduler lets it take each same\.txt before the next Offer, nothing coalesces, Sent reaches 1001, and the assertion st\.Sent &gt; 1000 fails on a correct execution\. This is unlikely given Run's tight loop, but it depends on scheduling, especially under \-race or with varying GOMAXPROCS\. The test's expectation is therefore not guaranteed by the code's contract\. Low severity is appropriate\. Evidence: fanout\.go:125\-129 \(Take deletes from waiting\); fanout\.go:220\-224 and 248\-311 \(concurrent sender\); fanout\_test\.go:87\-94 \(duplicates fed while the sender runs\); fanout\_test\.go:107 \(requires Sent &lt;\= 1000\)\. Static only\.


## Confirmed practice findings and optional suggestions

### Low · Exercise record removal and sender cleanup explicitly

**review-04-f02** · suggestion · shim/internal/shares/fanout\_test\.go:113

The test does not exercise the advertised microVM removal behaviour\. Missing coverage leaves sender termination, connection closure and removal of per\-VM state unverified when microVMs stop\.

**Source evidence:** fanout\_test\.go:67 claims that a microVM whose record disappears is forgotten, but lines 68\-113 never explicitly remove vm1\.args or assert cleanup\. fanout\.go:263\-279 contains distinct idle and failed\-dial removal paths; the supplied test uses only a successful connection\.

**Suggested change:** Add bounded cases that remove the record with an idle sender and with a queued event whose Dial fails; assert Snapshot loses the ID and any established connection closes\. Close both pipe endpoints through test cleanup\.

**judge-01 — confirmed:** The test's doc comment says a microVM whose record is gone is forgotten, but the test never removes vm1\.args and never asserts that the per\-VM state or the connection goes away\. The idle\-timeout and failed\-dial paths through alive/forget are unexercised\. The sender goroutine and the pipe ends also outlive the test\. This is a valid coverage gap for the lifecycle behaviour the request asks about; low severity as a suggestion\. Evidence: fanout\_test\.go:67 \(claim in comment\); fanout\_test\.go:68\-113 \(no removal of vm1\.args, no cleanup assertions\); fanout\.go:260\-283 and 314\-321 \(removal paths\)\.

### Low · Verify host metadata preservation alongside the no\-echo check

**review-04-f03** · suggestion · scripts/systemvm/microvm\-shares\-suite\.sh:99

The suite can report success if replay changes the host mtime once and then settles before q1\. Stable queue counters during the later observation window do not establish that replay preserved the host edit's timestamp\.

**Source evidence:** microvm\-shares\-suite\.sh:87\-93 performs edits, waits four seconds after the burst, then compares queue counters ten seconds apart; lines 98\-100 assert watcher counts and counter equality only\. guest\_linux\.go:267\-272 reads an mtime and subsequently writes it back, leaving an intervening host\-write window\. No host timestamp assertion is supplied\.

**Suggested change:** Add a controlled timestamp\-preservation case and a regression case that overlaps host writes with replay, checking final host contents, size and mtime as well as bounded echo activity\.

**judge-01 — confirmed:** The no\-echo check compares queued counters between q1 \(4 s after the burst\) and q2 \(10 s later\)\. A single echo caused by replay changing the host mtime, for example the race in review\-02\-f02, would arrive within the first 4 s and be included in q1\. It would then settle because the following replay rewrites the same value, so q1 \=\= q2 and the step passes\. The suite never asserts the host file's mtime or size after replay\. This is a valid gap in what the step proves; low severity is appropriate\. Evidence: microvm\-shares\-suite\.sh:86\-99 \(edits, 4 s wait, q1, 10 s, q2; assertions on watcher counts and q1 \=\= q2 only\); guest\_linux\.go:267\-272 \(sampled mtime written back\)\. No host timestamp assertion appears in the step\.


## Unverified or disputed claims

**Assessment: uncertain; requires further evidence.**

### Medium · Server discovery cache drops initial events for newly started microVMs

**review-02-f03** · defect · shim/internal/shares/fanout\.go:208

A microVM created within the refresh interval after the last server scan receives none of its host edits during that interval\. If an edit is the only subsequent input, its notification is permanently lost rather than delayed until discovery\.

**Source evidence:** Static evidence: fanout\.go:208\-213 refreshes the server list only when Offer is called and the cached scan is older than Refresh, which defaults to one second at lines 198\-199\. Lines 214\-238 map the current path only against that cached list\. Run at lines 349\-353 neither retains unmatched paths nor schedules a later discovery/replay\. Thus a newly present \.args file is ignored for an event arriving before cache expiry\. No checks were run\.

**Suggested change:** Discover current servers for each offered event, or retain events across discovery refreshes so newly started servers receive events that arrived during the cache interval\.

**judge-01 — uncertain:** The mechanism is real\. Servers are rescanned only when Offer runs and the cached scan is older than Refresh \(1 s\), and an unmatched path is not retained\. The claimed impact, however, requires the new microVM's guest to already have its NFS mounts set up by the time an event arrives inside that ≤1 s window after the \.args file appears\. Per the ADR, the record and server come before the jailer boots, and guest mounts happen only when the session's RUN arrives\. Until then the guest's Replay would find no mount and count the event as missing anyway\. Whether launch\-to\-mount can complete inside one refresh interval depends on when vesper\-microvm writes &lt;id&gt;\.args and on boot and session timing, neither of which is in the evidence\. Any loss is therefore plausible but likely confined to a period when delivery would not take effect anyway\. Evidence: fanout\.go:198\-200 and 208\-213 \(lazy 1 s refresh\); fanout\.go:214\-238 and 349\-353 \(no retention of unmatched paths\); ADR 0009 Lifecycle and Guest \(record and server before jailer; mounts at session RUN\)\. When the \.args file is written relative to guest mount readiness is not in the evidence\.

## Rejected and duplicate claims

- review-03-f01: rejected. In Linux, fsnotify\_change maps a setattr carrying ATTR\_MTIME without ATTR\_ATIME to FS\_MODIFY \(IN\_MODIFY\)\. IN\_ATTRIB is produced only when both ATTR\_ATIME and ATTR\_MTIME are set, the 'utime\(s\) call' case\. touchInRoot passes UTIME\_OMIT for atime and an explicit mtime, so only ATTR\_MTIME \(plus ctime\) is set and watchers get IN\_MODIFY\. That is the same mechanism the existing GH \#150 sink relies on \('touch \-c \-m \-r' after an atime\-only set\)\. The premise that utimensat always yields IN\_ATTRIB is incorrect for this call\.
- review-03-f02: duplicate of review-02-f02. This is the same Fstat\-to\-UtimesNanoAt window and the same backward\-mtime consequence as review\-02\-f02\.
- review-03-f03: duplicate of review-02-f04. This is the same unserialised writeStats publication through the shared temporary file as review\-02\-f04\.

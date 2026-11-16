# Council review

**Status: complete for the selected static scope.** 5/5 selected roles completed. 5 confirmed defect(s); 2 unverified or disputed claim(s).

**Request:** Review GH \#160 M4/M5 \(ADR 0009\): the containerd shim maps OCI bind mounts from macOS virtio\-fs shares to NFS exports of the microVM's sandboxed unfsd \(shim/internal/shares MakePlan, task service, backend Launch args\), and the guest \(Firecracker microVM, runs as root, the task's image is untrusted\) mounts them under /newroot with vesper\-console shares mount \(openat2 RESOLVE\_IN\_ROOT, mkdirat, mount on /proc/self/fd\), unmounting before the session reports EXIT\. Threat model: the task's image may hold symlinks designed to redirect a mount outside /newroot; bind sources are given by the containerd client \(trusted user\) but resolved in the System VM; nothing may expose a System VM path that is not under a share; read\-only binds must stay read\-only\. Press on: path resolution in MakePlan \(EvalSymlinks then prefix match against /proc/mounts entries, longest match, octal escapes, a share mounted inside another\); dedup of sources by \(rel, mode\) and the same dir bound both ro and rw; destination cleaning and nested destinations order; openInRoot's mkdir loop and races with the workload \(it has not started yet\); unmount order, EBUSY fallback and data loss; the guest\-session shell changes \(set \-e, trap, error message sanitising\); prepare's modules\.dep awk closure; vesper\-ctr run\-binds parsing; test validity of microvm\-shares\-suite \(does each step fail when the property breaks\)\.

**Evidence:** revisions; snapshot `eba61e15faca8c00e70eccf64db73ee86c51a906fcf96c5e3ee7109791613ed6`.

**Base:** `0a6c66c82fa0e494f283100f7c6acbf9d34bee1a`. **Head:** `2bd3134533d3a61d4d88e64cd6851f11cac99b56`.

**Verification:** static source assessment only. This pipeline did not execute repository code, tests, reproducers or reviewer-suggested commands. A completed review is not proof that the code is defect-free.

## Coverage

| Role | Runner | Status |
|---|---|---|
| security | codex:default | completed |
| correctness | codex:default | completed |
| reliability | codex:default | completed |
| testing | codex:default | completed |
| maintainability | codex:default | completed |


**Judges:** claude:opus: completed.

Judge independence is based on separate configured CLI providers and fresh sessions; model aliases do not prove distinct underlying models.


**Scope notes:**

- Static evidence only: no repository code, tests or suggested commands were executed\.
- Related context files are evidence, not automatic routing targets\.
- Some negative wording was not mapped to a role exclusion\. The original request remains authoritative; use \-\-exclude for an exact role constraint\.


## Confirmed defects

### High · SIGKILL has no bounded fallback for tasks blocked in hard NFS I/O

**review-02-f01** · defect · shim/internal/task/service\.go:304

If the share server stays unavailable while a task is blocked in hard NFS I/O, Kill only forwards the signal\. No timeout stops the VMM, so the guest may never report EXIT and Wait and task deletion can remain blocked indefinitely\.

**Source evidence:** Static evidence only: shares\.go:37\-38 selects hard NFS mounts\. service\.go:303\-307 forwards the signal and returns without scheduling a fallback; watch at 228 waits for the session, and Wait at 378\-381 waits for the exited state\. ADR 0009:128\-134 explicitly requires a bounded SIGKILL wait followed by stopping the VMM\. No such path appears in the supplied service source\.

**Suggested change:** After init SIGKILL, schedule a bounded wait outside the operation lock; stop the VMM and complete the exit transition if the guest does not report EXIT within the bound\.

**judge-01 — confirmed:** The scope is M4/M5, and ADR 0009 assigns 'the shim mapping and the kill path' to milestone 5\. Lifecycle requires that an init SIGKILL start a bounded wait for EXIT, outside the operation lock, and then stop the VMM\. The supplied Kill only forwards the signal and returns\. Nothing in the changed service code sets a timer or calls Stop\. With hard mounts and a server that stays down, the guest's unmount \(and therefore EXIT\) can block\. watch, Wait and Delete then depend on a session that may never finish\. The high severity holds only when the server stays down; supervise\-daemon respawns it within rate limits\. The MicroVM Signal implementation is not in the supplied files, but this diff does not change it, and the ADR places the bounded wait in the shim's kill handling\. Evidence: service\.go:303\-307 \(PhaseRunning: backend\.Signal, then return\); service\.go:228 \(watch blocks on Session\.Wait\); service\.go:378\-381 \(Wait\); shares\.go:37 \('hard'\); ADR 0009:128\-134 \(bounded wait, then vesper\-microvm stop\) and :230\-232 \(M5 includes the kill path\); vesper\-guest\-session:110\-111 \(unmount before EXIT\)\. The backend Signal implementation is not supplied\.

### Medium · Unmount records split valid destinations containing whitespace

**review-02-f02** · defect · shim/internal/shares/guest\_linux\.go:159

A valid destination such as /my data mounts successfully but is later interpreted as two destinations, /my and data\. Its NFS mount is not unmounted before EXIT, defeating the required flush and cleanup sequence\.

**Source evidence:** Static evidence only: shares\.go:42\-55 allows whitespace in destinations\. guest\_linux\.go:118 writes destinations separated by newlines, but line 159 reads them with strings\.Fields, which splits spaces, tabs and newlines\. vesper\-guest\-session:110 ignores the resulting unmount error and line 111 reports EXIT\.

**Suggested change:** Store and decode the destination list as JSON, preserving every pathname byte accepted by CheckDest\.

**judge-01 — confirmed:** CheckDest accepts destinations that contain spaces or tabs, and such OCI destinations are valid\. MountAll records destinations separated by newlines, but UnmountAll splits the record with strings\.Fields, which also splits on spaces\. A destination such as '/my data' is looked up as '/my' and 'data', so it is never unmounted\. The session ignores the unmount error and reports EXIT, which skips the flush\-before\-EXIT step the ADR requires\. Evidence: shares\.go:42\-55 \(no whitespace check\); guest\_linux\.go:118 \(strings\.Join\(done, "\\n"\)\); guest\_linux\.go:159 \(strings\.Fields\(string\(raw\)\)\); vesper\-guest\-session:110\-111 \('\|\| true', then echo EXIT\)\.

### Medium · run\-binds loses argument boundaries for paths containing spaces

**review-02-f03** · defect · systemvm/overlay/usr/local/bin/vesper\-ctr:160

A quoted bind argument containing a space in SRC or DST is split into multiple ctr arguments\. Valid share paths therefore fail or are parsed as different command arguments despite satisfying the documented restriction against only colons and commas\.

**Source evidence:** Static evidence only: vesper\-ctr:147 parses the original argument without splitting, but line 150 appends it to a scalar mounts string and line 160 expands \$mounts unquoted\. set \-f at line 143 disables globbing, not shell field splitting\. The interface restriction at lines 139\-140 does not exclude whitespace\.

**Suggested change:** Build ctr arguments using positional parameters or another argument\-preserving representation, keeping each \-\-mount value a single argument\.

**judge-01 — confirmed:** run\_binds builds every \-\-mount value into one scalar string and expands it unquoted\. set \-f only disables globbing, so default IFS field splitting still breaks a SRC or DST that contains spaces into several ctr arguments\. The documented restriction excludes only ':' and ','\. The impact is limited to this test and diagnostic helper and its suite; the default SHARE path has no spaces\. Evidence: vesper\-ctr:139\-140 \(restriction is ':' or ','\); :143 \(set \-f\); :150 \(mounts\="\$mounts \-\-mount type\=bind,src\=\$src,\.\.\."\); :160 \(unquoted \$mounts\)\.

### Medium · Lazy unmount is treated as proof that buffered writes have reached the server

**review-02-f04** · defect · shim/internal/shares/guest\_linux\.go:180

When normal unmount returns EBUSY, successful MNT\_DETACH is treated as completed cleanup even though lazy detachment does not wait for outstanding references and buffered NFS writes\. The session can report EXIT and subsequent Delete can kill the VMM and server before those writes finish\.

**Source evidence:** Static evidence only: guest\_linux\.go:178\-180 replaces EBUSY with the result of MNT\_DETACH; lines 160\-161 then remove the record on success\. vesper\-guest\-session:110\-111 reports EXIT immediately afterwards\. service\.go:335 invokes backend cleanup on Delete, and vesper\-microvm:706\-707 kills the VMM and stops unfs\. This failure requires a busy mount with outstanding writes; no runtime reproducer was run\.

**Suggested change:** Do not count lazy detachment as a completed flush\. Release remaining users and perform normal unmount, or explicitly complete and verify writeback before detachment; propagate cleanup failure instead of reporting an ordinary successful exit\.

**judge-01 — confirmed:** When the normal unmount returns EBUSY, the code falls back to MNT\_DETACH and treats success as completion\. It then removes the record, and the session reports EXIT\. A lazy detach does not wait for open references or their dirty pages to be written\. Delete then kills the VMM and stops unfsd, so data still held through a remaining reference can be lost\. This is conditional: a reference must survive the workload's exit\. Deepest\-first ordering avoids EBUSY from nested shares, and no reproducer was run\. Medium severity is reasonable\. Evidence: guest\_linux\.go:178\-181 \(EBUSY → Unmount\(target, MNT\_DETACH\)\); :160\-161 \(record removed on success\); vesper\-guest\-session:110\-111; service\.go:335 \(Cleanup on Delete\); vesper\-microvm:706\-707 \(kill \-KILL, unfs\_stop\)\.

### Low · Cleanup check treats failed server inspections as proof of absence

**review-04-f01** · defect · scripts/systemvm/microvm\-shares\-suite\.sh:68

The nothing\_left step can pass when a server inspection fails\. If leftovers reports CLEAN and an unfs query returns an error or no output, the absence of state\= is treated as successful cleanup, even though the server's absence was never established\.

**Source evidence:** Static evidence: lines 68–69 discard each query's status and accept any combined output without state\=\. The supplied vesper\-microvm source at lines 424–429 defines the expected absence response as UNFS &lt;id&gt; absent\. The leftovers implementation at vesper\-ctr lines 212–223 does not inspect unfs supervisor pids or cgroups, so it does not replace these queries\.

**Suggested change:** Check each query's exact expected UNFS &lt;id&gt; absent response, allowing its documented nonzero absence status, and fail on unexpected output or transport errors\.

**judge-01 — confirmed:** The nothing\_left step passes whenever leftovers reports CLEAN and the combined unfs query output does not contain 'state\='\. A failed ctl invocation, an ERR line or empty output would therefore count as 'server absent' without matching the documented 'UNFS &lt;id&gt; absent' response\. leftovers does not inspect unfs pid files or cgroups, so it does not cover this case\. This weakens the test's ability to detect a lingering server, but it is not a product defect, so low severity fits\. Evidence: microvm\-shares\-suite\.sh:68\-69; vesper\-microvm:424\-429 \('UNFS \$id absent'\); vesper\-ctr:212\-223 \(no unfs checks\)\.


## Confirmed practice findings and optional suggestions

### Low · Add regression cases for resolved source boundaries and mixed access modes

**review-04-f02** · suggestion · shim/internal/shares/shares\_test\.go:27

The supplied planning tests do not distinguish several security\-relevant regressions: matching a sibling path by a bare prefix, choosing an outer share over a nested share, classifying a symlink by its original rather than resolved location, or deduplicating the same directory across ro and rw modes\. This is missing coverage, not evidence that those implementations are defective\.

**Source evidence:** Static evidence: TestMakePlan lines 28–47 uses disjoint share roots, one symlink resolving within the same share, and duplicate sources only in the same mode\. TestShareMounts lines 101–106 verifies decoding a space but does not pass the nested mount points into MakePlan\.

**Suggested change:** Add table cases for a prefix sibling, symlinks resolving into and out of a share, nested share roots in both mount\-table orders, and one source bound both ro and rw\. Assert the selected mount, relative sources, export indices, flags and launch arguments\.

**judge-01 — confirmed:** This is accurately framed as a coverage gap\. TestMakePlan uses disjoint share roots, a symlink that resolves inside the same share, and duplicates only in the same mode\. No test covers a sibling with a bare prefix match, nested share mounts in either mount\-table order, a symlink resolving out of a share, or one directory bound both ro and rw\. TestShareMounts decodes a nested path but never passes it to MakePlan\. The request asked for these cases to be examined\. Evidence: shares\_test\.go:27\-55, :57\-76, :101\-107; shares\.go:152\-160 \(shareOf\), :217\-228 \(dedup on \(Rel, ReadOnly\)\)\.

### Low · Exercise guest destination confinement and cleanup failure paths

**review-04-f03** · suggestion · scripts/systemvm/microvm\-shares\-suite\.sh:28

The supplied integration suite exercises ordinary destinations and successful session completion, leaving the changed guest confinement and rollback contract without direct regression cases\. A regression in symlink confinement, partial\-mount rollback or unmount\-before\-EXIT handling could escape these checks\.

**Source evidence:** Static evidence: the suite uses /data and /data/conf at line 28 and no hostile image fixtures or injected mount/unmount failures\. The supplied Go tests cover planning and annotation helpers only\. guest\_linux\.go lines 43–77 implement symlink\-aware directory creation, lines 113–120 perform rollback, and lines 178–180 introduce the EBUSY detach fallback\.

**Suggested change:** Add targeted guest cases with absolute and parent\-traversing symlinks plus missing destination components, asserting mounts remain beneath the supplied root\. Add controlled partial\-mount and cleanup failures, checking rollback and the session's EXIT ordering; include a busy mount case that verifies the intended write\-preservation behaviour\.

**judge-01 — confirmed:** This is a valid coverage gap for properties the request names explicitly\. The suite uses benign destinations \(/data, /data/conf\) and has no hostile image symlinks\. It injects no mount or unmount failures and has no busy\-mount case\. The Go tests cover only planning and annotation helpers\. So regressions in openInRoot confinement, rollback, or the EBUSY/MNT\_DETACH path would not be caught\. The finding is a suggestion and does not claim a defect\. Evidence: microvm\-shares\-suite\.sh:28\-75; guest\_linux\.go:43\-77, :113\-120, :178\-180; shares\_test\.go \(no guest\_linux tests\)\.


## Unverified or disputed claims

**Assessment: uncertain; requires further evidence.**

### Medium · TCP readiness probe does not bound a stalled NFS mount

**review-03-f03** · defect · shim/internal/shares/guest\_linux\.go:138

A server can accept TCP connections while failing to answer NFS or mount RPCs, or stop responding immediately after the probe\. In that sequence probe succeeds and the subsequent synchronous hard mount can block without the three\-second bound, preventing the session from reaching READY or its cleanup trap until external teardown\.

**Source evidence:** probe returns success immediately after a TCP connection opens \(guest\_linux\.go:85\-88\)\. mountOne then calls unix\.Mount synchronously \(138\), using hard mounts \(shares\.go:37\)\. No RPC responsiveness check or cancellation mechanism surrounds that call\. vesper\-guest\-session waits for this command before emitting READY \(66\-72\)\. External session timeout implementation is not supplied\.

**Suggested change:** Bound the actual mounting operation through supervised execution and explicit timeout recovery; a TCP connection check alone must not be presented as bounding mount completion\.

**judge-01 — uncertain:** It is true that probe only checks that a TCP connection opens and that the mount\(2\) call that follows has no timeout of its own\. Whether a hard NFSv3 mount syscall can block indefinitely on a server that accepts TCP but does not answer RPCs depends on the kernel's mount\-time RPC timeouts and retry behaviour, which the supplied evidence does not establish\. The ADR accepts a port probe as the design for this bound and mentions an outer 30 s session deadline\. That deadline's implementation is not supplied, and neither is any evidence of how the guest recovers\. The gap is plausible, but its impact is not shown\. Evidence: guest\_linux\.go:81\-94 \(TCP dial only\); :138 \(synchronous unix\.Mount\); shares\.go:37 \('hard'\); ADR 0009:154\-155 \(port probe, 30 s deadline\)\. Kernel behaviour and the session deadline code are not supplied\.

**Assessment: uncertain; requires further evidence.**

### Medium · Propagate failures when reading the module dependency closure

**review-05-f01** · defect · systemvm/overlay/usr/local/bin/vesper\-microvm:341

If modules\.dep is missing or unreadable, awk fails but prepare continues with an empty files list\. It can publish and stamp an initramfs without the required kernel modules, breaking subsequent microVM boots rather than reporting the preparation failure\.

**Source evidence:** Static evidence: lines 335–341 assign awk output without checking its exit status\. The script enables only set \-u \(line 37\)\. An empty result skips the copy loop at lines 342–346; lines 355–357 then build and stamp the initramfs\. This failure condition is conditional; the supplied evidence does not establish that modules\.dep is currently missing\.

**Suggested change:** Check the awk assignment's exit status and return an explicit preparation error before building or stamping the initramfs\.

**judge-01 — uncertain:** The awk assignment's exit status is not checked, and the script runs only with set \-u\. However, the claimed outcome \(an empty file list\) is not established\. Common awk implementations report an unreadable input file and still run END, which would print the start set without its dependencies\. The failure also needs modules\.dep to be missing or unreadable, and the old code already tolerated a missing modules\.dep\. The lack of error propagation is real, but the described impact and the medium severity are speculative\. Evidence: vesper\-microvm:335\-341 \(unchecked command substitution\); :37 \(set \-u only\); :342\-346, :355\-357; base :331\-333 \(modules\.dep copied conditionally\)\.

## Rejected and duplicate claims

- review-03-f01: duplicate of review-02-f02. Same root cause and impact as review\-02\-f02: destinations with whitespace are split by strings\.Fields in UnmountAll\.
- review-03-f02: duplicate of review-02-f04. Same issue as review\-02\-f04: MNT\_DETACH after EBUSY is treated as a completed flush before EXIT and VM teardown\.
- review-03-f04: duplicate of review-02-f03. Same unquoted \$mounts field\-splitting defect as review\-02\-f03\.
- review-05-f02: duplicate of review-02-f03. Same unquoted \$mounts field\-splitting issue in run\_binds as review\-02\-f03\.

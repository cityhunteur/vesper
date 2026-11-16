# Council review

**Status: complete for the selected static scope.** 5/5 selected roles completed. 7 confirmed defect(s); 2 unverified or disputed claim(s).

**Request:** Review GH \#160 M3 \(ADR 0009\): a per\-microVM unfs3 server sandbox\. vesper\-unfs run \(Go\) opens share sources with openat2 \(RESOLVE\_BENEATH\|NO\_SYMLINKS\|NO\_XDEV\), clones them into detached mount trees, starts init in new mount/pid/ipc/uts namespaces, attaches the trees into a tmpfs root, binds read\-only sources again inside read\-write ones, pivot\_root, drops to root with 6 capabilities \(or uid 65534\), no\_new\_privs, a seccomp filter against user namespaces/setns/clone3, then execs unfsd\. vesper\-microvm \(BusyBox ash, runs as root\) starts it under supervise\-daemon in a cgroup inside the microVM's network namespace\. Threat model: the guest of that microVM is hostile and may exploit unfsd; it must not read or write anything outside the exported sources, nor write a read\-only source \(including one nested in a read\-write source\); macOS processes may swap share directories for symlinks at any time; containers and other microVMs must not reach the server\. Press on: TOCTOU between validation and binding; whether any path is re\-resolved after openat2; nest placement under concurrent symlink swaps; the seccomp BPF program's correctness \(jump offsets, arch check, clone flag argument on arm64 and x86\_64, clone3, other ways to get a user namespace or new mounts\); capability and no\_new\_privs ordering; descriptors leaking into unfsd; the root tmpfs and /usr, /lib, /dev/null binds; supervise\-daemon lifecycle, cleanup and cgroup kill; the probe hook temporarily adding 169\.254\.77\.2 on lo and an ACCEPT rule; shell quoting of sources with spaces and metacharacters; test validity \(does each suite step fail if the property is broken\)\.

**Evidence:** revisions; snapshot `4e506bf7f6a7f8ec5262c83a78f85c78dedcd8900f87fe647d11e5a00127cf70`.

**Base:** `f275b6928f8d87887c1c2480f957cc1adcd06124`. **Head:** `6b74754d3bf0cf7dcddf55b3c119f6d30b2fda0b`.

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

### High · Concurrent directory renames can leave the read\-only source writable through its outer export

**review-01-f01** · defect · shim/internal/unfs/sandbox\_linux\.go:294

With an outer rw source proj and inner ro source proj/conf, a macOS process can rename conf to conf\-old and create a replacement conf after the inner source descriptor is opened but before nest placement\. The read\-only tree still refers to the original directory, but placement resolves the replacement\. The original directory remains reachable and writable as /s/0/conf\-old\. An exploited unfsd can therefore write the read\-only source without gaining mount privileges\.

**Source evidence:** Static evidence: lines 85–94 open sources independently and retain their descriptors; lines 114–119 clone the opened inner directory read\-only\. Lines 294–302 subsequently resolve n\.Sub again beneath the outer export and attach that clone to the newly resolved directory, without checking its identity against the opened inner source\. RESOLVE\_NO\_SYMLINKS rejects symlinks but permits a replacement real directory\. Nests is computed from path strings in config\.go:103–115\.

**Suggested change:** Establish nested protection using the pinned inner directory's identity within the pinned outer tree, rather than resolving its original pathname again\. Fail closed if the relationship cannot be established under concurrent renames\. Add a regression scenario that renames the opened inner directory and installs a replacement before nest attachment\.

**judge-01 — confirmed:** The nest is placed by resolving a path again, and nothing checks that the resolved directory is the one that was opened\. OpenSources opens the outer source \(proj\) and the inner source \(proj/conf\) as separate descriptors, then clones the inner one read\-only\. Later, Init resolves the lexical subpath n\.Sub beneath the attached outer tree and moves the read\-only clone onto whatever directory it finds there\. RESOLVE\_NO\_SYMLINKS blocks symlinks but not a real replacement directory\. If a macOS process renames conf to conf\-old and creates a new conf between the openat2 in OpenSources and the openat2 in Init, the clone covers the new directory\. The original conf stays reachable and writable as /s/0/conf\-old through the rw outer mount\. The threat model says macOS processes may swap share directories at any time, and the requirement is that a nested read\-only source is never writable\. Respawns repeat the whole sequence, so the window recurs\. High severity is supported for a race whose timing depends on a host process\. Evidence: sandbox\_linux\.go:85\-94 opens each source separately\. sandbox\_linux\.go:114\-119 runs detach\(dirs\[n\.Inner\], \.\.\.\|MOUNT\_ATTR\_RDONLY\)\. sandbox\_linux\.go:290\-302 runs Openat2\(outer, n\.Sub, RESOLVE\_BENEATH\|NO\_SYMLINKS\|NO\_MAGICLINKS\), then moveTree\(firstFd\+len\(cc\.Sources\)\+k, at\), with no identity comparison \(for example st\_dev/st\_ino or a mount ID\) against the pinned inner descriptor\. config\.go:88\-115 derives Sub from the path strings alone\. No test or reproducer was run\.

### Medium · Source reconstruction performs shell pathname expansion

**review-02-f02** · defect · systemvm/overlay/usr/local/bin/vesper\-microvm:368

Sources containing glob characters can be changed or multiplied before reaching Go\. If a newline\-delimited source such as project\*:rw matches filenames in the script's working directory, ash expands it into those filenames, exporting unintended sources or rejecting an otherwise valid literal directory name\.

**Source evidence:** Static evidence: line 368 expands \$us\_srcs unquoted in a for loop\. Setting IFS to newline at line 367 preserves spaces but does not disable pathname expansion\. ParseSource and CheckRel accept glob characters \(config\.go:44–83\)\. The quoted "\$us\_s" occurs only after expansion has happened\.

**Suggested change:** Read the newline\-delimited sources with IFS\= read \-r, or disable pathname expansion while splitting and restore its previous setting afterwards\.

**judge-01 — confirmed:** Line 368 expands \$us\_srcs unquoted in a for list\. With IFS set to a newline, field splitting keeps spaces intact, but ash still performs pathname expansion on each field\. ParseSource and CheckRel accept '\*', '?' and '\[' in components\. Any source containing a glob that matches a path relative to the manager's working directory is replaced by the matching names before vesper\-unfs sees it\. The request asked specifically about shell quoting of metacharacters, and this is a concrete mishandling\. Likelihood is low: a match needs names in the current directory that end in ':ro' or ':rw'\. Go also still confines the result beneath the share mount with openat2, so the effect is a wrong or extra export inside the share, or a failed launch, not an escape\. The test\_unfs\_selfcheck hook already uses the safe \`IFS\= read \-r\` pattern\. Evidence: vesper\-microvm:367\-369: \`us\_ifs\=\$IFS; IFS\=\$NL; for us\_s in \$us\_srcs; do set \-\- "\$@" \-\-src "\$us\_s"; done\`, with no \`set \-f\`\. vesper\-microvm:489\-491 rejects only empty or newline\-containing \-\-unfs\-src values\. config\.go:65\-83 permits glob characters\. Compare vesper\-microvm:442 \(\`while IFS\= read \-r line\`\)\.

### Medium · Probe interruption leaves the guest address assigned to loopback

**review-02-f03** · defect · systemvm/overlay/usr/local/bin/vesper\-microvm:470

If the probe hook is terminated after configuring loopback, its cleanup is skipped\. The guest address remains local to the server namespace, disrupting the guest's connection to its NFS server until the address is removed or the namespace is recreated\.

**Source evidence:** Static evidence: lines 463–464 add the guest address and an INPUT ACCEPT rule\. Their removal occurs only after the foreground probe returns, at lines 471–472\. This command installs no signal or exit cleanup trap; the launch trap at line 526 is installed only by launch\. The hook's comment explicitly identifies the guest connectivity interruption\.

**Suggested change:** Run the hook in a scope with EXIT and signal cleanup traps, track successful additions, and always remove the temporary address and rule\.

**judge-01 — confirmed:** test\_unfs\_probe adds 169\.254\.77\.2/32 on lo and inserts an INPUT ACCEPT rule for lo\. It removes both only after the foreground probe returns\. Nothing in the function or at script level installs a trap; the only trap is in launch\(\)\. If TERM, INT or HUP arrives while the probe runs, the address and rule stay behind\. The guest address then sits locally in the server namespace, so the server's replies to the guest are routed to lo, and the hook's own comment says the guest's connection cannot work in that state\. The impact is limited to a test hook, but it is a hook shipped in the production manager script and it breaks a live microVM's shares until cleaned up\. Medium severity is acceptable\. Evidence: vesper\-microvm:463\-464 run \`ip addr add "\$SHARE\_GUEST/32" dev lo\` and \`iptables \-w \-I INPUT 1 \-i lo \-j ACCEPT\`\. vesper\-microvm:470\-472 run the probe and only then delete both\. The comment at vesper\-microvm:456\-458 says "the guest's own connection to the server cannot work meanwhile"\. The only trap in the script is at line 526, inside launch\.

### Medium · Read\-only probes count transport failures as successful refusals

**review-02-f04** · defect · shim/internal/nfsprobe/battery\.go:129

A server that disconnects or stops answering during the read\-only checks can still receive passing results for those checks\. Consequently failed\=0 does not establish that the server returned read\-only refusals for the attempted operations\.

**Source evidence:** Static evidence: the refused helper at lines 128–130 accepts every non\-nil error, including RPC connection errors, as success\. It is used for read\-only creates, writes, chmod, mkdir and remove at lines 131–151\. Failed lookups also skip several write checks at lines 133 and 147\. The suite accepts the failed\=0 summary at unfs\-suite\.sh:67 and 122\.

**Suggested change:** Require a successfully decoded NFS refusal status for read\-only operations, fail transport errors, and explicitly fail prerequisite lookups instead of skipping their dependent checks\.

**judge-01 — confirmed:** The refused helper passes whenever err \!\= nil\. That includes connection resets, timeouts, short replies and RPC AcceptErrors such as GARBAGE\_ARGS, as well as genuine NFS refusals\. A server that dies or drops the connection during the read\-only section therefore scores every remaining refusal as a pass\. Some dependent checks are also skipped without being counted\. If the Walk of r2/r\.txt fails, the write and chmod checks are skipped\. If the Lookup of conf/app\.conf fails, the write and remove checks are skipped\. So failed\=0, which the suite treats as success, does not prove those read\-only refusals happened\. The request asks whether each suite step fails when the property is broken, and this is a valid test\-validity defect\. Evidence: battery\.go:128\-130 defines refused as \`b\.check\(name, err \!\= nil \|\| st \!\= 0, \.\.\.\)\`\. battery\.go:133 is \`if fh, \_, st0, \_ :\= n\.Walk\(r2, "r\.txt"\); st0 \=\= 0 \{\`, with no else branch\. battery\.go:147 is \`if fh, \_, st1, \_ :\= n\.Lookup\(conf, "app\.conf"\); st1 \=\= 0 \{\`, with no else branch\. unfs\-suite\.sh:67 and :122 accept \`\*"failed\=0"\*\`\.

### Medium · Forged\-handle checks pass when the connection fails before coverage completes

**review-04-f01** · defect · shim/internal/nfsprobe/battery\.go:251

A transport error on any forged variant ends the loop but still reports success when outside remains zero\. Thus even a failure on the first variant produces a passing forged\-handle check, and the remaining variants are never exercised\.

**Source evidence:** Static evidence: battery\.go:237\-241 breaks on any Getattr error without recording a failure; line 251 checks only outside \=\= 0\. Lines 197\-200 reuse the same connection for the subsequent file lookup, so a dropped connection can also skip the entire file\-handle battery\.

**Suggested change:** Reconnect after an expected server restart and continue the remaining variants\. Record unexpected transport errors as failures, and explicitly fail or report incomplete coverage when variants or the file\-handle battery cannot be exercised\.

**judge-01 — confirmed:** In forged\(\), any Getattr error causes a break that only logs an INFO line\. The final check tests only outside \=\= 0\. A connection failure on the first variant therefore yields a passing 'forged' check with almost no coverage\. The same NFS connection is then reused for the Walk of file\.txt and for the second forged\(\) run, so after a drop that battery is skipped silently or passes vacuously\. The code says the drop is the expected TD\-065 abort, but the suite cannot tell "server asserted on variant k" from "variants never exercised"\. The request asks whether each step fails when the property is broken, and this is a valid test\-validity defect\. Evidence: battery\.go:237\-241: \`if err \!\= nil \{ b\.info\("forged\-dropped "\.\.\.\); break \}\`\. battery\.go:251: \`b\.check\("forged "\+name, outside \=\= 0, \.\.\.\)\`\. battery\.go:197\-200 reuse n for \`n\.Walk\(r0, "file\.txt"\)\` and the second forged\(\) call, with no reconnect\.

### Medium · UID 1000 readability assertion checks ownership text instead of read success

**review-04-f02** · defect · scripts/systemvm/unfs\-suite\.sh:72

The step named probe\_as\_uid\_1000\_sees\_its\_own\_files can pass when reading secret600 is denied or returns incorrect contents\. Its condition checks only the reported ownership, while the battery deliberately excludes this read from its failure count\.

**Source evidence:** Static evidence: unfs\-suite\.sh:71\-72 matches INFO read\-0600 containing uid\=1000 gid\=1000\. battery\.go:111\-117 emits that information regardless of Read status, error or contents, without calling b\.check\. Consequently failed\=0 does not establish this step's claimed readability\.

**Suggested change:** Make the UID 1000 run assert successful reading of secret600 with the expected contents, using a counted probe check or an explicit structured result consumed by the suite\.

**judge-01 — confirmed:** The step probe\_as\_uid\_1000\_sees\_its\_own\_files passes when the summary contains failed\=0 and the INFO read\-0600 line contains 'uid\=1000 gid\=1000'\. The INFO line comes from b\.info, which is not counted\. It prints the attributes and the read status, error and data, but nothing requires the read to succeed or return 's'\. A denied or wrong read would still pass the step, as long as the attribute text contains the uid and gid\. The battery's comment says the result is 'Reported, not judged', which contradicts the step's name\. Evidence: unfs\-suite\.sh:71\-72: \`own\=\.\.\. grep '^INFO read\-0600'\` and \`case "\$\{sum\}:\$\{own\}" in \*"failed\=0"\*":"\*"uid\=1000 gid\=1000"\*\)\`\. battery\.go:111\-117 print the read\-0600 result through b\.info, with no b\.check on status or data\.

### Medium · Forged\-handle containment oracle accepts any object on the share filesystem

**review-04-f03** · defect · shim/internal/nfsprobe/battery\.go:245

A forged handle resolving to an unexported directory or file on the same virtio\-fs filesystem is counted as safe\. This prevents the probe from detecting the specific escape to m3/outside that its fixture is intended to expose\.

**Source evidence:** Static evidence: battery\.go:243\-247 increments outside only when FSID differs from shareFS\. The suite creates m3/outside/secret\.txt beside the exported directories at unfs\-suite\.sh:37\-42\. Such an object shares the filesystem identity of the exports; the exported FileID map populated at battery\.go:158\-161 is never consulted\. The comment at lines 218\-220 assumes sandbox containment instead of independently testing it\.

**Suggested change:** Add an independent outside\-source sentinel check, such as deliberately constructed handles for known unexported fixture objects, and assert that getattr/read/list operations refuse them\. Do not treat matching FSID alone as proof of export containment\.

**judge-01 — confirmed:** The forged\-handle oracle counts an accepted variant as a violation only when its FSID differs from that of /s/0\. The fixture's unexported m3/outside/secret\.txt sits on the same virtio\-fs filesystem as the exports and would have the same FSID\. A forged handle that reached it, or any other unexported directory in the share, would be counted as safe\. The exported FileID map is filled in but never consulted\. The code comment relies on the sandbox to provide containment instead of testing it\. The sandbox's subtree bind mounts may well prevent this escape, but the probe cannot detect it if they do not, so the finding about test validity stands\. Evidence: battery\.go:243\-247: \`if st \=\= 0 \{ accepted\+\+; if a\.FSID \!\= b\.shareFS \{ outside\+\+ \.\.\. \} \}\`\. battery\.go:158\-161 populate b\.exported, which is never read\. battery\.go:218\-220 contain the comment that assumes containment\. unfs\-suite\.sh:37 and :41 create m3/outside/secret\.txt beside the exported directories\.


## Confirmed practice findings and optional suggestions

None recorded.


## Unverified or disputed claims

**Assessment: uncertain; requires further evidence.**

### Medium · The amd64 filter permits x32 variants of the namespace syscalls

**review-01-f02** · defect · shim/internal/unfs/seccomp\_linux\.go:57

On an amd64 kernel with the x32 ABI enabled, an exploited unfsd can use x32 syscall numbers to bypass the clone, unshare and setns checks\. x32 uses the same AUDIT\_ARCH\_X86\_64 value but adds \_\_X32\_SYSCALL\_BIT to syscall numbers\. These numbers fall through to ALLOW, permitting creation of a user namespace where kernel policy otherwise allows it\. This defeats the stated namespace restriction; the supplied evidence does not establish a complete filesystem escape\.

**Source evidence:** Static evidence: lines 54–56 accept the configured audit architecture\. Lines 57–63 compare the syscall number only with native unix\.SYS\_\* values, and line 67 allows unmatched numbers\. There is no rejection or normalisation of \_\_X32\_SYSCALL\_BIT\. seccomp\_linux\_test\.go:62–70 covers native syscall numbers and a different audit architecture, but not x32 numbers sharing the accepted architecture\.

**Suggested change:** For amd64, reject syscall numbers carrying \_\_X32\_SYSCALL\_BIT before the native syscall comparisons, and add filter\-interpreter cases for x32 clone, unshare and setns\.

**judge-01 — uncertain:** The filter mechanism is as described\. On amd64, x32 syscalls report AUDIT\_ARCH\_X86\_64 with \_\_X32\_SYSCALL\_BIT set in nr\. The filter compares nr only against native numbers and allows anything unmatched, so x32 unshare, clone and setns would pass\. The impact, however, depends on prerequisites the evidence does not show\. First, the deployed System VM appears to be aarch64: vesper\-microvm copies ld\-musl\-aarch64\.so\.1, and the filter's arm64 path has no x32 equivalent\. Second, the kernel would need CONFIG\_X86\_X32\_ABI enabled\. Third, the kernel would need to allow unprivileged user namespaces\. Even then, mounts inherited into a new user namespace are locked, so the gain beyond the namespace restriction itself is not established\. The defect is real for an amd64 build with x32 enabled, but medium severity for this deployment is not supported\. Evidence: seccomp\_linux\.go:54\-67 compares the loaded nr only against unix\.SYS\_CLONE3/SETNS/UNSHARE/CLONE and returns ALLOW otherwise\. There is no check of the 0x40000000 bit\. seccomp\_linux\_test\.go:62\-70 has no x32 case\. vesper\-microvm:305 and :311 reference ld\-musl\-aarch64\.so\.1, which indicates an arm64 System VM\. Neither the amd64 kernel configuration nor the deployment target is supplied\.

**Assessment: uncertain; requires further evidence.**

### Medium · An old sandbox error can make subsequent launches fail immediately

**review-03-f03** · defect · systemvm/overlay/usr/local/bin/vesper\-microvm:386

After a launch fails with a source or sandbox error, retrying the same ID with corrected arguments can fail before its new server has time to listen\. The readiness loop treats the previous launch's retained error line as a failure from the current launch, triggering cleanup of an otherwise valid new server\.

**Source evidence:** Static evidence: vesper\-microvm:371 removes pid and err files but does not reset the server log\. Lines 383\-386 check listening once and then search the entire retained log for any vesper\-unfs run/init error\. unfs\_stop at lines 392\-402 and cleanup\_locked at lines 680\-683 do not remove the unfs log, including for cleanup \-\-logs\.

**Suggested change:** Before starting the new supervisor, rotate or truncate its log, or limit startup error detection to output produced by the current launch\.

**judge-01 — uncertain:** The script side matches the description\. unfs\_start does not remove or rotate /var/log/vesper\-unfs\-&lt;id&gt;\.log\. unfs\_stop and cleanup\_locked do not remove it either, even with \-\-logs\. The readiness loop greps the whole log for a \`vesper\-unfs run\|init:\` line after each failed listen check\. Whether a stale line survives into the next launch depends on vesper\-boundlog: if it appends to an existing file, a retry of the same ID would break out on the first iteration, before the new server listens\. vesper\-boundlog's behaviour is not supplied\. The explicit \`rm \-f "\$lf"\` before boundlog in launch \(line 588\) hints that it appends, but this is not proven\. Supervise\-daemon's handling of an existing log is also not shown\. Evidence: vesper\-microvm:371 removes only the \.pid and \.err files\. vesper\-microvm:383\-387 check unfs\_listening, then run \`grep \-q "^vesper\-unfs \\\(run\\\|init\\\):" "/var/log/vesper\-unfs\-\$us\_id\.log" &amp;&amp; break\`\. vesper\-microvm:392\-402 \(unfs\_stop\) and :683 \(cleanup \-\-logs\) do not remove /var/log/vesper\-unfs\-\$id\.log\. The source of vesper\-boundlog is missing\.

## Rejected and duplicate claims

- review-02-f01: duplicate of review-01-f01. This is the same defect as review\-01\-f01\. Nest placement in Init re\-resolves n\.Sub and attaches the pinned read\-only clone onto a replacement directory, while the original remains writable under another name through the rw outer export\.
- review-03-f01: duplicate of review-01-f01. This is the same rename race in nest placement as review\-01\-f01\.
- review-03-f02: duplicate of review-02-f03. This is the same missing cleanup trap in test\_unfs\_probe as review\-02\-f03\.
- review-05-f01: duplicate of review-02-f02. This is the same unquoted pathname expansion of \$us\_srcs at vesper\-microvm:368 as review\-02\-f02\.

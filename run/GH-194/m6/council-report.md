# Council review

**Status: complete for the selected static scope.** 5/5 selected roles completed. 4 confirmed defect(s); 1 unverified or disputed claim(s).

**Request:** Review GH \#194 milestone 6 \(ADR 0010\): vesper\-data legacy import\-volume \(copy a pre\-remap Docker volume into the remapped store with every uid/gid shifted \+100000\), vesper\-data trim, the Docker vsock relay restricted to the macOS host with max\-children\=256, and the upstream\-suite runner changes\. Everything in the overlay runs as root inside an Alpine System VM under BusyBox ash\. Threat model: a hostile
container \(remapped, possibly \-\-privileged within its userns\) must not gain host\-root\-equivalent files or reach
dockerd; only the macOS host may reach the Docker relay\. Questions to press on:
1\. import\-volume: can a crafted volume \(symlinks, hard links to files outside the volume, device nodes, names, ids
   at boundaries 65535/65536, setuid files, filenames with newlines or leading dashes\) make the root\-run copy or
   chown/chmod touch anything outside \.import\-NAME, or leave a file owned by real root in the remapped store?
2\. The SHIFT\_SCRIPT under \`find \-exec env \.\.\. sh \-c\`: quoting, symlink handling \(chown \-h?\), and the hard\-link
   SEEN set; partial failure and cleanup of \.import\-NAME; the lock; the race between \`docker volume create\` and mv\.
3\. trim: discard detection per device, and whether fstrim can run against the wrong mount\.
4\. The relay: can a non\-host vsock peer, or a container, reach docker\.sock through any path; is the cap correct
   for socat; what happens to the relay when the cap is hit\.
5\. upstream\-suite run\.sh: the /dev/vd&lt;letter&gt; derivation when boot media or rootfs counts change\.
6\. Do the docs \(DEVELOPMENT\.md, SECURITY\.md, TESTING\.md, READMEs, design\-v0\.md Phase 4\) match the code?

**Evidence:** revisions; snapshot `ca354286769acfc18443902f2b3ff7b1bd91e4d6f5b8c98ead24ffda961d56a4`.

**Base:** `2ec62814e511cd1be5c2118c14e942da62e75035`. **Head:** `cd7063a91e454bfbbfaa4485fe50bf3aabdeadaa`.

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

### High · Ownership shifting exposes the staging tree to container\-controlled path replacement

**review-01-f01** · defect · systemvm/overlay/usr/local/bin/vesper\-data:175

During an authorised import, a hostile remapped container with the System VM filesystem bound into it can replace paths in the staging tree after their directories become owned by uid 100000\. Subsequent root\-run stat, chown or chmod operations can then resolve through attacker\-installed intermediate symlinks outside \.import\-NAME\. This can change a root\-only System VM file to mapped ownership, allowing the container to read or modify it\. The import lock excludes other importers, not container filesystem writes\.

**Source evidence:** vesper\-data:201 preserves source modes and ownership with cp \-a; lines 202 and 168\-175 traverse the copy and change directory ownership without isolating it from containers\. A legacy root\-owned directory with mode 0755 becomes writable by container root when shifted to 100000\. Line 175 uses chown \-h, which protects only the final symlink component; intermediate components still resolve normally\. Lines 169 and 176 also perform separate pathname lookups\. docs/SECURITY\.md:343\-345 explicitly permits containers to receive a bind of /\. Static evidence only; exploiting the window requires concurrent path replacement during import\.

**Suggested change:** Create a root\-owned mode\-0700 staging parent and place both the copied tree and inode markers beneath it\. Keep that parent inaccessible to mapped users throughout copying and ownership shifting, then publish the completed tree by rename\. Ensure concurrent containers cannot bind or otherwise access staging descendants while the import runs\.

**judge-01 — confirmed:** The staging copy is created directly under the remapped store's volumes directory with a predictable name, \`\.import\-NAME\`\. There is no private root\-only parent\. The SHIFT\_SCRIPT runs in find's pre\-order, so the top staging directory, and each directory below it, is chowned to BASE\+uid \(for example 100000\) before its descendants are processed\. From then on, container root owns that directory and can write to it\. Docs and the ADR say containers can reach other volumes through /var/lib/docker/100000\.100000/volumes/\.\.\., which supports the claim that a container given / can traverse to the staging path\. A hostile container can therefore replace a descendant directory with a symlink, for example to /etc\. This works between the chown of the parent and the processing of already\-batched descendant paths\. The root\-run stat, \`chown \-h\` and chmod then resolve intermediate components through that symlink, outside \`\.import\-NAME\`\. \`chown \-h\` only protects the final component\. Because the attacker also controls the legacy volume's names and ids, it can choose descendant names that match target System VM files, such as shadow\. The result is a System VM file re\-owned into the mapped range and readable or writable by the container\. The import lock only excludes other importers\. The race needs a container running during an authorised import, and a tight loop of renames makes it practical\. High severity is justified under the stated threat model\. Evidence: vesper\-data:197\-202: \`tmp\="\$NEW/volumes/\.import\-\$name"\`, \`cp \-a "\$src/\_data" "\$tmp"\`, \`find "\$tmp" \-exec env \.\.\. sh \-c "\$SHIFT\_SCRIPT" sh \{\} \+\`\. SHIFT\_SCRIPT:169 \`stat \-c \.\.\. "\$p"\`, :175 \`chown \-h "\$u:\$g" "\$p"\`, :176 \`chmod "\$m" "\$p"\`; all are path\-based\. docs/SECURITY\.md:343\-345 and ADR 0010:45\-46 say containers given / read other volumes under /host/var/lib/docker/100000\.100000/volumes\. The exact directory modes of /var/lib/docker/100000\.100000 and volumes/ are not in the snapshot; traversal rests on those documented statements\. Nothing was executed\.

### Medium · Inode\-marker creation failures silently leave hard\-linked files unshifted

**review-02-f02** · defect · systemvm/overlay/usr/local/bin/vesper\-data:172

If mkdir fails for a hard\-linked inode for a reason other than an existing marker, such as exhausted space or inodes, every name for that inode is skipped without reporting E\. The import can then succeed with the original ownership, including real root ownership, violating the ownership guarantee\.

**Source evidence:** Static evidence only: line 172 treats every failed mkdir as an already\-seen inode and continues before chown\. Lines 204\-206 reject only explicit E output\. The free\-space estimate at lines 193\-195 does not reserve or verify free inodes and cannot prevent allocation failures\. Thus a failed marker creation is indistinguishable from successful prior processing\.

**Suggested change:** Skip only when a marker already exists; emit E and fail the import for other marker\-creation failures\. Mark an inode as completed only after its ownership and mode updates succeed\.

**judge-01 — confirmed:** Line 172 treats any mkdir failure as 'already seen' and continues before chown, without printing E\. A marker mkdir that fails for another reason \(ENOSPC, exhausted inodes, EIO\) silently skips that name\. Every other name of the same inode fails the same way, so the inode keeps the original owner copied by \`cp \-a\`, which can be 0:0\. Lines 204\-206 count only E lines, so the import is published with IMPORTED\. That leaves a file owned by real System VM root in the remapped store, which contradicts the guarantee in SECURITY\.md:363\-365\. The pre\-check at 193\-195 counts KiB only, not inodes\. It needs an allocation failure during the walk, which is uncommon, so medium severity is reasonable\. Evidence: vesper\-data:172 \`if \[ "\$4" \-gt 1 \] &amp;&amp; \[ \! \-d "\$p" \]; then mkdir "\$SEEN/\$5" 2&gt;/dev/null \|\| continue; fi\`; :204\-206 reject only \`^E\$\`; :193\-195 check free blocks only\. SECURITY\.md:364\-365: 'no imported file is owned by a System VM uid outside the mapping'\. Nothing was executed\.

### Medium · Concurrent volume creation can overwrite or delete a volume the import did not create

**review-02-f03** · defect · systemvm/overlay/usr/local/bin/vesper\-data:208

If another Docker client creates the same volume after the initial inspect, docker volume create can return the existing volume successfully\. The importer then replaces its empty \_data directory, or attempts to remove that unrelated volume on failure\. The import lock only serialises imports and does not protect Docker clients\. A further recreation of \_data between rmdir and mv can make mv nest the imported directory while reporting success\.

**Source evidence:** Static evidence only: the existence check is at line 190, before the potentially lengthy copy and shift at lines 201\-202\. Lines 208\-211 neither establish exclusive creation nor prevent use of the volume: they call idempotent docker volume create, rmdir and ordinary mv, then docker volume rm on failure\. There is no coordination with Docker volume consumers in the supplied implementation\.

**Suggested change:** Coordinate publication with Docker so the destination cannot be concurrently created or mounted, and track ownership of the newly created volume before cleanup\. Use rename semantics that fail if the destination reappears instead of allowing mv to nest the staging tree\.

**judge-01 — confirmed:** Line 190 checks that the volume does not exist, then the import takes the lock, copies and shifts, which can take a long time\. Docker's local driver treats \`docker volume create\` of an existing name as idempotent and returns success, so line 208 does not prove this import created the volume\. If another client created the same name in the meantime and wrote data into it, \`rmdir "\$dst"\` fails\. The rollback then runs \`docker volume rm "\$name"\` on a volume the importer did not create; dockerd refuses only if the volume is in use\. If the competing volume's \_data is still empty, the import silently places its data into that volume\. The competing creator must be a Docker API client\. Containers cannot reach dockerd, so in practice this is the trusted macOS user racing themselves\. Likelihood is low, but the data\-loss path is accurately described from source\. The sub\-claim about \`mv\` nesting the copy if \_data reappears between rmdir and mv is theoretically valid for an existing\-directory target, but nothing shown would recreate \_data\. Evidence: vesper\-data:190 \`docker volume inspect\` before the lock \(191\) and copy \(201\-202\); :208 \`docker volume create "\$name"\`; :210\-211 \`rmdir "\$dst" &amp;&amp; mv "\$tmp" "\$dst"\` and on failure \`docker volume rm "\$name"\`\. The idempotent volume\-create behaviour is Docker's documented behaviour; it is not shown in the snapshot\. Nothing was executed\.

### Medium · Release the import lock and temporary copy on termination

**review-03-f03** · defect · systemvm/overlay/usr/local/bin/vesper\-data:192

Terminating an import with TERM or HUP can leave its mkdir lock behind for the rest of the boot, causing every subsequent import to report that another import is running\. The trap also does not clean the temporary copy, so interruption during a large copy can retain substantial persistent disk space\. There is no owner record or stale\-lock recovery in the supplied implementation\.

**Source evidence:** Lines 191–192 acquire a directory lock and register only an EXIT trap that removes the lock\. No TERM, HUP or INT handler is installed\. Temporary paths are assigned at lines 197–198 and cleaned only on selected explicit failure paths; the EXIT trap does not remove them\. Line 191 rejects every existing lock without checking whether its owner survives\. Static evidence only\.

**Suggested change:** Install explicit termination handlers that exit through cleanup, and make cleanup remove unpublished temporary paths and release the owned lock\. Prefer a process\-lifetime flock, or record ownership and provide safe stale\-lock recovery for termination that cannot run traps\.

**judge-01 — confirmed:** The lock is a directory created by mkdir and released only by an EXIT trap \(line 192\)\. No owner is recorded and there is no stale\-lock recovery \(line 191 rejects any existing lock\)\. SIGKILL can never run the trap\. POSIX/ash shells generally do not run EXIT traps on untrapped fatal signals, so TERM or HUP likely leaves the lock as well; for BusyBox ash this needs runtime confirmation\. One way this happens: the vsock client drops during a long \`cp \-a\` and socat terminates its child\. After that, every import reports 'another import is running' until /run is cleared at reboot\. The trap also never removes \`\$tmp\` or \`\$seen\`, so a partial copy stays on the persistent data disk until a later import of the same name runs \`rm \-rf\` at line 199\. Impact is availability and disk use, not security, so medium is at the high end but defensible\. Evidence: vesper\-data:191 \`mkdir "\$LOCK" 2&gt;/dev/null \|\| \{ echo "ERR another import is running"; \.\.\.\}\`; :192 \`trap 'rmdir "\$LOCK" 2&gt;/dev/null' EXIT\`; LOCK\=/run/vesper/legacy\-import\.lock \(137\); tmp and seen \(197\-198\) are cleaned only on explicit failure paths\. BusyBox ash signal and EXIT\-trap behaviour was not verified at runtime\.


## Confirmed practice findings and optional suggestions

### Low · Extend import coverage to adversarial entries and ID boundaries

**review-04-f01** · suggestion · scripts/systemvm/confine\-suite\.sh:197

The supplied import test covers ordinary owners, one out\-of\-range owner, an internal symlink, an internal hard link and setgid preservation\. It does not establish containment for outward or dangling symlinks, source hard links outside the volume, device nodes, unusual filenames, or the 65535/65536 boundary\. Regressions in those cases could escape the current assertions; this is a coverage gap, not evidence of an exploit\.

**Source evidence:** vesper\-data:233\-239 plants IDs 0, 1000 and 70000, setgid mode 2755, a symlink to root\.txt and a hard link to root\.txt\. confine\-suite\.sh:197\-203 checks only that fixture's selected metadata and output\. No adversarial fixture or boundary assertion appears in the supplied tests\.

**Suggested change:** Add fixture cases for IDs 65535 and 65536 independently for uid and gid, setuid files, outward and dangling symlinks, an external source hard link, device nodes, and filenames containing newlines or leading dashes\. Assert all imported ownership falls within the mapping and external sentinel contents, ownership and modes remain unchanged\.

**judge-01 — confirmed:** This is an accurate coverage gap, labelled as a suggestion with low severity\. The plant fixture uses ids 0, 1000 and 70000, one setgid file, an internal symlink and an internal hard link\. It has no 65535/65536 boundary for uid or gid, no setuid file, no outward or dangling symlink, no device node and no unusual filename\. Given review\-01\-f01 and review\-02\-f02, adversarial fixtures are relevant\. Evidence: vesper\-data:233\-239 \(plant\), confine\-suite\.sh:197\-203 \(assertions limited to that fixture\)\.

### Low · Cover failed imports, cleanup and competing volume creation

**review-04-f02** · suggestion · scripts/systemvm/confine\-suite\.sh:199

The new suite exercises successful import, duplicate import and one invalid name, but does not exercise partial failures or concurrency\. It therefore cannot detect regressions that publish an incompletely shifted copy, retain a lock or scratch directory, or remove a volume created by another caller during import\.

**Source evidence:** confine\-suite\.sh:189\-204 invokes a successful import, repeats it, and rejects \.\./etc\. vesper\-data:191\-212 contains separate lock, copy, ownership\-shift, Docker\-create and rename failure paths, including a Docker volume removal on placement failure\. The supplied test has no failure injection or concurrent creator\.

**Suggested change:** Add controlled failures during copy, owner shifting, Docker creation and final placement; assert no imported volume is published, scratch state is removed, and a subsequent import can acquire the lock\. Add a competing volume\-create case and verify the competing volume and its contents survive\.

**judge-01 — confirmed:** Accurate: step 7b covers a successful import, a duplicate import and one invalid name only\. The failure and rollback paths, lock release, and concurrent\-create behaviour in import\_volume are not exercised\. Low severity is appropriate for a test\-coverage suggestion\. Evidence: confine\-suite\.sh:189\-204; vesper\-data:191\-213 failure branches\.

### Low · Exercise the Docker relay through the real listener and at its connection cap

**review-04-f03** · suggestion · tests/vsock\_peer\.rs:53

The added test verifies rejection with manually supplied environment values, but does not verify that the listener supplies the expected peer identity or that the new nested relay preserves host traffic and half\-close behaviour\. It also leaves the 256\-connection limit and recovery untested\.

**Source evidence:** tests/vsock\_peer\.rs:53\-61 starts the handler directly for CID 3 and a missing peer address\. confine\-suite\.sh:123\-131 accepts transport\-level denial, so those probes need not reach the handler\. vesper\-docker\-relay:14 adds both EXEC and max\-children\=256, while vesper\-docker\-relay\-conn:8 introduces another socat hop\.

**Suggested change:** Add a transport integration case with a successful CID\-2 request, a non\-host rejection where the handler is actually reached, and a half\-closed request receiving its complete response\. Hold 256 connections, check bounded acceptance, then release one and verify service resumes\. No such execution evidence was supplied here\.

**judge-01 — confirmed:** Accurate\. The new Rust test runs vesper\-docker\-relay\-conn directly with environment variables it sets itself\. confine\-suite step 4 counts transport\-level refusals \(reset or no device\), so the handler may never be reached by a non\-host peer\. Nothing exercises a successful CID\-2 relay through the new EXEC plus nested\-socat path, half\-close behaviour across the extra hop, or max\-children\=256 saturation and recovery\. This is a coverage suggestion, not a demonstrated defect\. Evidence: tests/vsock\_peer\.rs:51\-62; confine\-suite\.sh:122\-131; vesper\-docker\-relay:14; vesper\-docker\-relay\-conn:8\.

### Low · Add focused regression cases for trim and disk\-device derivation

**review-04-f04** · suggestion · scripts/systemvm/upstream\-suite/run\.sh:244

The supplied tests do not cover the new device derivation or trim contract\. A regression could select the wrong vesper\_data device or consult discard support for a device different from the mounted filesystem without being caught by the shown import and peer tests\.

**Source evidence:** run\.sh:244\-246 derives the device from the presence of boot\_media\_path and rootfs\_path and replaces existing command\-line tokens\. vesper\-data:99\-102 checks mount presence, reads discard support using the command\-line device, and invokes fstrim on MNT\. The supplied evidence contains no assertions for these branches; the referenced attachment implementation is not supplied\.

**Suggested change:** Add table\-driven config cases for all boot\-media/rootfs presence combinations, absent and stale vesper\_data tokens, and duplicate mode tokens\. Add trim cases for an absent mount, unsupported discard, fstrim failure, successful output parsing, and a mismatch between the configured device and mounted source\.

**judge-01 — confirmed:** Accurate coverage gap\. Nothing shown tests the run\.sh device derivation from boot\_media\_path and rootfs\_path, or trim's branches\. trim reads discard support from the cmdline device but runs fstrim on \$MNT, so a mismatch would go unnoticed\. The ordering contract in src/systemvm/data\_disk\.rs is not supplied, so whether the derivation is correct cannot be checked statically here\. Low severity is fitting\. Evidence: run\.sh:244\-246; vesper\-data:98\-106; data\_disk\.rs is not in the snapshot\.


## Unverified or disputed claims

**Assessment: uncertain; requires further evidence.**

### Medium · Scratch directory names prevent importing valid long volume names

**review-05-f01** · defect · systemvm/overlay/usr/local/bin/vesper\-data:198

Valid volume names of 241–255 ASCII characters pass validation but cannot be imported on ext4\. The inode\-tracking directory adds 15 characters to the name, exceeding ext4's 255\-byte component limit, so mkdir fails before copying\.

**Source evidence:** Static evidence: lines 140–143 explicitly accept volume names up to 255 characters\. Lines 197–198 construct \.import\-NAME and \.import\-NAME\.inodes; the latter adds 15 bytes\. Line 200 requires that directory to be created\. Lines 60–64 establish ext4 as the data filesystem\.

**Suggested change:** Use a bounded, unique scratch directory name independent of the full volume name, with separate data and inode\-tracking children\. Preserve support for the documented 255\-character volume names\.

**judge-01 — uncertain:** The mechanism is real\. valid\_name accepts up to 255 characters\. \`\.import\-NAME\.inodes\` adds 15 bytes, and \`\.import\-NAME\` adds 8, so names of 241 characters or more exceed ext4's 255\-byte component limit, and \`mkdir "\$seen"\` fails with 'ERR cannot make'\. However, the failure is clean and fails closed, with no data loss or security impact\. It affects only extreme\-length names: a 255\-character volume name already approaches path limits elsewhere\. Medium severity is overstated for what is a low\-impact edge\-case limitation\. Evidence: vesper\-data:141\-143 \(accepts names up to 255 characters\); :197\-198 \(prefix and suffix added\); :200 \(\`mkdir "\$seen" \|\| \{ echo "ERR cannot make \$seen"; return 1; \}\`\); ext4 at :64\.

## Rejected and duplicate claims

- review-02-f01: duplicate of review-01-f01. Same issue as review\-01\-f01\. The staging tree under the remapped volumes directory becomes owned by container root partway through the walk\. Its intermediate components can then be swapped for symlinks, so the later root\-run stat, chown or chmod acts outside \`\.import\-NAME\`\.
- review-03-f01: duplicate of review-02-f02. The central example, a SEEN marker mkdir failing on exhausted inodes and silently skipping a hard\-linked inode, is the same defect as review\-02\-f02\. The added point is also correct: line 202 ignores find's exit status\. It is a minor extension of the same 'failure is not reported as E' problem\.
- review-03-f02: duplicate of review-02-f03. Same TOCTOU as review\-02\-f03\. The idempotent \`docker volume create\` can adopt a volume created concurrently, and the rollback's \`docker volume rm\` can delete it\.

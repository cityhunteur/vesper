# GH #160: carry host shares into the shim's microVMs over NFS from `unfs3`

This is workstream W5 of `docs/exec-plans/active/2026-11-13-outstanding-work-programme.md` (D-029). The GH #159
prototype (D-020) found one share path into a Firecracker microVM that works, NFSv3 from the userspace server
`unfs3` in the System VM over a tap device, and the owner approved `unfs3` as a runtime dependency on three
conditions (D-021). This plan turns the prototype into the shim's bind-mount path.

## Purpose / Big Picture

After this change, `docker run -v /Users/me/src:/src …` on the Vesper runtime puts the macOS directory at `/src`
inside the task's microVM, as it already does for runc containers. Reads, writes back to macOS with the sharing
user as owner, read-only shares, and host edits seen on the next open all work, with the deviations D-020 measured:
`stat` can lag up to 1 s, and I/O is 3 to 11 times slower than virtio-fs. No Docker container, and no other
microVM, can reach the file server or another microVM's export.

## Progress

- [x] M0 Design: ADR 0009 (accepted, `docs/design-docs/adr/0009-shares-into-microvms-over-nfs.md`), reviewed by two
  llm-council design reviews before any code. First review 2026-11-13 (4 High, 5 Medium, 5 Low); second, with a
  comparison of alternatives, the same day (2 High, 8 Medium, 6 Low). All are in the ADR and the milestones, and the
  owner accepted it with those fixes (D-032)
- [x] M1 (2026-11-14, D-033) `unfs3` pinned in `docs/specs/systemvm-packages.lock.yaml` and listed in `THIRD_PARTY_NOTICES.md`, with its
  dependency closure, each licence and the uid-dropping tool; anything beyond `unfs3` goes to the owner in this
  PR; no server starts at boot
- [x] M2 (2026-11-14) Share network per shim microVM with shares: `netns_up` and `netns_down` moved to the sourced
  `/usr/local/lib/vesper/netns.sh` (TAP owner uid and state directory as parameters, `vesperd` unchanged); a
  namespace under `/run/vesper/microvm/netns/` holding `lo` and one TAP owned by uid 10000 (`169.254.77.1/30`
  server, `.2` guest) and the in-namespace `INPUT` rule, joined by the jailer with `--netns`; a per-id record
  written under one lock before the namespace (the lock not held over the boot poll, its descriptor closed in
  children), an error trap in `launch`, and the `vesper-microvm reap` pass (which runs `cleanup`) in a new
  `vesper-microvm-reaper` service; `docs/SECURITY.md` and `design-v0.md` updated here, when shim microVMs first get a
  network device
- [ ] M3 One `unfsd` per microVM in that namespace, sandboxed (new mount, pid, ipc and uts namespaces, `pivot_root`
  to a tmpfs, sources at `/s/N` with `nosuid,nodev` and kernel `ro`, `no_new_privs`, a reduced capability bounding
  set), in a capped cgroup admitted by `vesper-memadmit` with the microVM, under `supervise-daemon`, logging through
  `vesper-boundlog`; sources opened under the share mount's descriptor and bound from it; every `ro` source bound
  again read-only inside each `rw` source that contains it; memory measured; the non-root `all_squash` server
  adopted if `0600` files, `0700` directories and the binds suite work with it, else root with the capability set;
  tests: a container and another microVM cannot connect, a `..`, symlink or forged handle finds nothing outside
  the exports, a handle cannot cross between exports, a nested-chroot escape fails, a write to a `ro` export fails,
  a `ro` source nested in a `rw` one cannot be written through the outer export, a source swapped for a symlink
  after validation exports nothing outside the share, `ESTALE` after a restart measured. Exit gate: the GH #159
  `>` then `>>` lost write is reproduced and explained, or shown not to recur in 1,000 tries
- [ ] M4 Guest: NFS client modules and their `modules.dep` closure, and `virtio_net`, in the initramfs;
  `vesper_share_net=` brings up `eth0` with no default route; the share list arrives as JSON in the session's `RUN`
  request; a `vesper-console` subcommand decodes it and calls `mount(2)` for each share, shallowest destination
  first, after a 3 s probe of the server, with
  `addr=169.254.77.1,vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive`
  (+ `ro`); the guest unmounts its shares before it reports `EXIT`
- [ ] M5 Shim: OCI bind mounts whose resolved source is under a share mount become NFS mounts; file sources are
  refused with a clear error; other bind mounts keep today's behaviour; `Kill` forwards the signal as today, and a
  `SIGKILL` whose `EXIT` does not arrive within a bounded wait (outside the task's operation lock) stops the VMM;
  no `SIGTERM` timer; the server stops at `PhaseStopped`; the leak sensors cover servers and namespaces
- [ ] M6 Event hop: the GH #150 host-edit replay reaches the microVM, so a macOS edit gives `IN_MODIFY` inside it,
  through a bounded, coalescing queue per microVM, with no echo back to macOS
- [ ] M7 `binds-suite` variant against the microVM path, with the failure tests below; deviations recorded as
  accepted or fixed; `design-v0.md`, `docs/SECURITY.md`, TD-032, TD-049 and the decision log updated; #160 closed

## Surprises & Discoveries

- 2026-11-14 (M1): `unfs3` 0.10.0-r2 brings seven more packages: `rpcbind`, `libtirpc`, `libtirpc-conf`, `krb5-libs`,
  `krb5-conf`, `libverto` and `keyutils-libs` (Alpine's `libtirpc` links Kerberos), about 3 MB installed. `rpcbind` is
  installed because the package depends on it; nothing starts it, and `unfsd -p` never registers with it.
- 2026-11-14 (M1): BusyBox `setpriv` has neither `--reuid` nor `--bounding-set`, so it can drop neither the server's
  uid nor its capabilities. util-linux `setpriv` 2.41.6 (with `libcap-ng`) does both and installs at `/bin/setpriv`,
  replacing the applet, which nothing in the overlay used. `run/GH-160/m1-packages.txt` shows uid 65534 with an
  empty bounding set, and root with exactly the ADR's six capabilities (`CapBnd` `0xdb`), both with `NoNewPrivs`.
- 2026-11-14 (M1): regenerating the lock from today's Alpine indexes would also move `runc` from 1.3.4-r1 to
  1.3.7-r0. The old package is still published, so its entry is kept as it was; a `runc` bump is its own change.
- 2026-11-14 (M2): busybox `tunctl -u` takes a user name, not a uid, and no user had uid 10000. `vesper-microvm
  setup-user` creates `vesper-shim-jail` (the reaper service does it at boot, and `launch --share-net` checks it).
- 2026-11-14 (M2): `nsenter --net` does not change what `/sys` shows, so the `sharenet` sensor first listed the root
  namespace's interfaces. It mounts a fresh sysfs inside the namespace.
- 2026-11-14 (M2): the guest side of the network (`vesper_share_net=`, planned for M4) was needed to prove that the
  microVM's own guest reaches the server address and another guest does not, so the init configures `eth0` now.
  M4 keeps the NFS modules and the mounts.
- 2026-11-14 (M2): the reaper also skips an id whose shim still runs. A stopped task keeps its jail until the shim
  deletes it, and reaping on "Firecracker gone" alone would take it from under a live shim.
- 2026-11-14 (M2): `make systemvm-sharenet-suite` passes 10 of 10 (`run/GH-160/m2-sharenet-suite.json`): with a test
  listener in one microVM's namespace, its guest reaches port 2049 and not 2051; the root namespace, another
  namespace, another guest, and privileged containers on the bridge and on the host network do not.
- 2026-11-14 (M2): the regression run found an older fault. `cleanup` looked for leftover sockets with
  `find /run -name "*ID*"`, and containerd names shim sockets by a hex digest, so a short id such as `ca` often
  matched another task's socket. The false `DIRTY` skipped the memory release (W4 releases only on `CLEAN`), the
  entry stayed for 30 s, and a suite that started straight after `shim-suite` was refused for memory. The check now
  matches the id as a whole word. From a clean boot `sharenet`, `shim`, `workload`, `microvm`, `launchapi`,
  `vesperd` and `egress` suites pass back to back, and the ledger is empty after them.
- 2026-11-14 (M1): after boot with the new overlay no `unfsd` or `rpcbind` runs, no OpenRC service names them, and
  nothing listens on 111, 2049 or 2050. `shim-suite` (22 of 22) and `workload-suite` (16 of 16) pass.

## Decision Log

- 2026-11-13: one server per microVM, inside that microVM's own network namespace (ADR 0009), not one server in the
  root namespace bound to a share address. A container can route to any System VM address whatever the server binds,
  so the root-namespace design would rest on filter rules; the per-namespace design leaves no NFS listener a container
  can route to, and meets D-021's three conditions by construction. The same fixed address pair serves every
  namespace, so no allocator is needed.
- 2026-11-13: the council design review (Codex, Claude, Antigravity, judged by Opus) found no route by which a
  container or another microVM reaches a server, and four High gaps, all taken into ADR 0009: (1) a chroot does not
  hold a root `unfsd`, so the sandbox is `pivot_root` with new namespaces, `no_new_privs` and a reduced capability
  set, proved by an escape test; (2) with `hard` mounts a dead server makes a guest process unkillable, so `Kill`
  stops the VMM; (3) no reaper or lock existed for shim microVMs and the launch window would look orphaned, so a
  record is written under a lock first; (4) `vesperd`'s namespace helpers make TAPs for the wrong uid and are
  reaped by `vesperd`, so they move to a shared file with the owner and directory as parameters. The Medium and
  Low findings (kernel `ro`, `modules.dep`, generated export paths and a JSON share list, sensors, the System VM
  target, a separate kernel parameter, the derived threat table, respawn limits, a capped cgroup, an in-namespace
  `INPUT` rule) are in the ADR and the milestones. If the sandbox needs `CAP_SYS_ADMIN` or `CAP_SYS_CHROOT`, the
  design goes back to the owner, and the vsock file service is the fallback.
- 2026-11-13: M0 comes first. Like TD-058 (ADR 0008), this opens a network path from a guest to a service in the
  System VM, so its isolation rules are decided and reviewed before code. Anything that would let a guest reach more
  than its own exports goes back to the owner.

- 2026-11-13: a second council review (same seats), asked also to compare alternatives, kept the design: no
  alternative beat a server per microVM in a private namespace. FUSE over vsock stays the fallback (best correctness,
  3,000 to 5,000 new lines); NFS relayed over vsock is slower and risks a writeback deadlock; 9p stays excluded; a
  block image cannot carry writes; a custom memory-safe NFS server is possible later. Its two High findings changed
  the ADR: a `ro` source nested in a `rw` source was writable through the outer export, so `ro` sources are bound
  again inside it; and stopping the VMM on every `SIGKILL` lost buffered writes, so `Kill` signals the guest first
  and stops the VMM only after a bounded wait. The Mediums are in too: the non-root `all_squash` server is preferred
  but must first read `0600` files (shares appear as `0:0`); sources are bound from a held descriptor; mounts are
  done by `vesper-console` from the `RUN` request after a server probe; the server's memory is admitted with the
  microVM's; #160 closes only with the event hop. One finding was out of date: `vesper-memadmit` exists since W4.
  The report is a working paper (`run/` is not tracked); its conclusions are here and in the ADR.
- 2026-11-13: the owner accepted ADR 0009 with those fixes (D-032). M1 can start.
- 2026-11-14 (M2 council review, no blocker, 2 Medium, 11 Low): (1) the lock covered only the record and the
  namespace, so a reaper pass could give back a new launch's memory admission; every launch now holds the lock from
  its already-running check to its namespace, clears an earlier run's record and namespace first, and `cleanup`
  takes the lock. (2) A process with the shim's name on its command line could hold the reaper off an orphan; a
  shim now counts by its binary's device and inode and an exact `-id`, and a Firecracker by its cgroup. Also fixed:
  the reaper logs errors and prints `KEPT` for a removal that was not clean; `cleanup` validates the id and the
  socket check treats `-` as part of a word; the record write is checked; `launch --share-net` waits for the
  guest's interface; the `sharenet` sensor cannot report `fc_in_ns=yes` from two empty reads. Three suite steps
  were added. Left as TD-060: the jailer pid in `launch`, and pruning reaped microVMs' logs.
- 2026-11-14: a container given the System VM's root (`--privileged` with a host bind mount or the host pid
  namespace) can enter any namespace there. That is outside ADR 0009's threat table, which covers containers
  confined by runc; such a container already reads every share directly. `docs/SECURITY.md` states the boundary.
- 2026-11-14 (owner, D-033): the dependency closure of `unfs3` is approved as listed in Surprises, and the sandbox
  tool is util-linux `setpriv`, because one tool covers both the non-root server and the root fallback with a
  reduced bounding set. `su-exec` covers only the first; a helper of our own would be new privileged code.

## Outcomes & Retrospective

Not yet written.

## Context and Orientation

- The shim (`shim/`, Go) runs each containerd task as a jailed Firecracker microVM through
  `systemvm/overlay/usr/local/bin/vesper-microvm`. Today those microVMs have no network device (TD-032).
- `vesperd` microVMs already get taps in their own network namespace (CE-0010, D-027) and an optional NAT uplink
  (ADR 0008). The share network should reuse that namespace pattern, not the uplink.
- Host shares reach the System VM over virtio-fs (`systemvm-bringup.md`, GH #29). The launcher replays macOS edits
  as inotify events in the System VM (GH #150).
- The prototype's evidence and numbers are in `docs/exec-plans/completed/2026-11-11-GH159-share-into-microvm-prototype.md`.
  The kernel `nfsd` is ruled out: it returns stale file handles for every new directory over virtio-fs.

## Plan of Work

The milestones above, in order, each a PR. M0 is a document and a review, and no code changes until it is accepted.
M2 and M3 carry the security tests. M6 may land after M7's suite, but #160 stays open until its event test passes,
unless the owner changes that acceptance criterion.

## Concrete Steps

    make systemvm-overlay && make systemvm-up-shares
    make binds-suite                          # today's Docker path, unchanged
    make binds-suite VESPER_RUNTIME=microvm   # the new variant (M7)

## Validation and Acceptance

#160's acceptance criteria, each shown by a suite step:
- `unfs3` is pinned and supervised and is reachable only on the share network.
- A container cannot connect.
- A microVM mounts only its own exports.
- Writes reach macOS with the right owner, and read-only shares are enforced.
- A macOS edit gives `IN_MODIFY` in the microVM.
- The binds-suite variant passes with the recorded deviations.

Failure tests, each a suite step:
- With the server killed and held down, `docker stop` (and `ctr task kill -s KILL`) ends the task within its grace
  period plus 10 s, and `Delete` completes.
- With the server alive, `dd` of 32 MiB to a share without `fsync`, then `docker kill`: the file on macOS has all
  32 MiB.
- A stalled server at session start fails the `RUN` within 5 s with an error naming the share.
- The server respawns under load and I/O resumes; a `launch` racing `reap` leaves either a running task or nothing.
- `0600` files and `0700` directories read and write through the share.
- A file 60 directories deep reads, and a write through a descriptor opened before a rename across directories
  either succeeds or is recorded as a deviation.
- A burst of 1,000 macOS edits reaches the guest coalesced, with the queue bounded and no echo.
- After a `launch` aborted mid-way, and after the shim is killed, the reaper leaves `vesper-ctr leftovers` clean
  (no server, supervisor, namespace or exports file).
- A write to a read-only mount fails in the guest and through a forged handle, and macOS sees no change.
- A share whose path contains spaces and shell characters mounts and reads correctly.
- The nested-chroot escape fails inside the sandbox.

## Idempotence and Recovery

Exports and share networks are per microVM, and `vesper-microvm cleanup` removes them. The `vesper-microvm reap`
pass, under the same lock as `launch`, removes any whose record is past its grace period and whose Firecracker is
gone; it never touches `vesperd`'s namespaces, which live in another directory. `unfs3` restarts under `supervise-daemon`. Removing the package and the shim's mapping
returns to today's behaviour, in which shares reach runc containers only.

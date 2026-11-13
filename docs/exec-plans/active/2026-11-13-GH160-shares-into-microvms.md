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

- [ ] M0 Design: ADR 0009 (proposed, `docs/design-docs/adr/0009-shares-into-microvms-over-nfs.md`), reviewed by an
  llm-council design review before any code. First review done 2026-11-13 (4 High, 5 Medium, 5 Low, all taken into
  the ADR); the revision awaits the owner's acceptance
- [ ] M1 `unfs3` pinned in `docs/specs/systemvm-packages.lock.yaml` and listed in `THIRD_PARTY_NOTICES.md`; no
  server starts at boot
- [ ] M2 Share network per shim microVM with shares: `netns_up` and `netns_down` moved to the sourced
  `/usr/local/lib/vesper/netns.sh` (TAP owner uid and state directory as parameters, `vesperd` unchanged); a
  namespace under `/run/vesper/microvm/netns/` holding `lo` and one TAP owned by uid 10000 (`169.254.77.1/30`
  server, `.2` guest) and the in-namespace `INPUT` rule, joined by the jailer with `--netns`; a per-id record
  written under one lock before the namespace, an error trap in `launch`, and the `vesper-microvm reap` pass
- [ ] M3 One `unfsd` per microVM in that namespace, sandboxed (new mount, pid, ipc and uts namespaces, `pivot_root`
  to a tmpfs, sources at `/s/N` with `nosuid,nodev` and kernel `ro`, `no_new_privs`, a reduced capability bounding
  set), in a capped cgroup, under `supervise-daemon`; memory measured; `all_squash` evaluated; tests: a container
  and another microVM cannot connect, a `..`, symlink or forged handle finds nothing outside the exports, a handle
  cannot cross between exports, a nested-chroot escape fails, a write to a `ro` export fails, `ESTALE` after a
  restart measured
- [ ] M4 Guest: NFS client modules and their `modules.dep` closure, and `virtio_net`, in the initramfs;
  `vesper_share_net=` brings up `eth0` with no default route; the share list arrives as JSON over the vsock
  session; the init mounts each share with
  `vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive` (+ `ro`)
- [ ] M5 Shim: OCI bind mounts whose resolved source is under a share mount become NFS mounts; file sources are
  refused with a clear error; other bind mounts keep today's behaviour; `Kill` with `SIGKILL`, or after the
  `SIGTERM` grace period, stops the VMM; the server stops at `PhaseStopped`; the leak sensors cover servers and
  namespaces
- [ ] M6 Event hop: the GH #150 host-edit replay reaches the microVM, so a macOS edit gives `IN_MODIFY` inside it
- [ ] M7 `binds-suite` variant against the microVM path, with the failure tests below; deviations recorded as
  accepted or fixed; `design-v0.md`, `docs/SECURITY.md`, TD-032, TD-049 and the decision log updated; #160 closed

## Surprises & Discoveries

None yet.

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
M2 and M3 carry the security tests. M6 can slip to a follow-up if M1 to M5 and M7 land with the deviation recorded.

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

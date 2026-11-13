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
  llm-council design review before any code
- [ ] M1 `unfs3` pinned in `docs/specs/systemvm-packages.lock.yaml` and listed in `THIRD_PARTY_NOTICES.md`; no
  server starts at boot
- [ ] M2 Share network per shim microVM with shares: a namespace holding `lo` and one TAP device
  (`169.254.77.1/30` server, `.2` guest), joined by the jailer with `--netns`; nothing else in it
- [ ] M3 One `unfsd` per microVM in that namespace, in a private mount namespace chrooted to its bind-mount sources,
  under `supervise-daemon`; started at launch and stopped at cleanup and by the reaper; a container and another
  microVM cannot connect, and a `..` or forged file handle finds nothing outside the exports (tests)
- [ ] M4 Guest: NFS client modules in the initramfs; the init mounts each share with
  `vers=3,proto=tcp,port=2049,mountport=2050,mountproto=tcp,nolock,hard,actimeo=1,lookupcache=positive`
- [ ] M5 Shim: OCI bind mounts of shared paths become NFS mounts; other bind mounts keep today's behaviour
- [ ] M6 Event hop: the GH #150 host-edit replay reaches the microVM, so a macOS edit gives `IN_MODIFY` inside it
- [ ] M7 `binds-suite` variant against the microVM path; deviations recorded as accepted or fixed; docs; #160 closed

## Surprises & Discoveries

None yet.

## Decision Log

- 2026-11-13: one server per microVM, inside that microVM's own network namespace (ADR 0009), not one server in the
  root namespace bound to a share address. A container can route to any System VM address whatever the server binds,
  so the root-namespace design would rest on filter rules; the per-namespace design leaves no NFS listener a container
  can route to, and meets D-021's three conditions by construction. The same fixed address pair serves every
  namespace, so no allocator is needed.
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

    make systemvm-overlay && make systemvm-up-snapshot
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

## Idempotence and Recovery

Exports and share networks are per microVM, and `vesper-microvm cleanup` removes them. A reaper pass removes any
whose microVM is gone. `unfs3` restarts under `supervise-daemon`. Removing the package and the shim's mapping
returns to today's behaviour, in which shares reach runc containers only.

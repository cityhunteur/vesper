I recommend deferring TD-067 and doing the other three: an I/O bound on the share network for TD-069, and both GH #207 and GH #208, each with a fix to the issue's sketch. The two most important findings are a read-only bypass that the #207 sketch would open, and a second depth limit (1,024-byte paths) that TD-067 does not record. Everything below is from reading the code ([S]) unless marked [E]. The only checks I ran were downloading the unfs3 0.10.0 tarball (digest matches `sources.sha256`) and `go test` for `internal/shares` and `internal/unfs` (pass, cached).

## 1. TD-067 (deep paths): defer, keep it accepted

- **ARC-1** [S] The cap is the protocol's handle size, not a Vesper limit:
  - NFSv3 allows at most 64 bytes per handle (`fh.h` `FH_MAXBUF`). After 21 fixed bytes (dev, ino, gen, pwhash, len), 43 one-byte hashes remain, one per path component counted from `/` (`fh.c:241-252`).
  - `/s` and `/s/N` use two of them, which gives the 40 levels in TD-067.
  - `fh_extend` returns NULL when the handle is full (`fh.c:289`).
- **ARC-2** [S] A second limit is not recorded anywhere: unfs3 caps every path at 1,024 bytes (`nfs.h:28` `NFS_MAXPATHLEN`). Real deep trees such as `node_modules` reach about 1,000 bytes by level 40, so raising the handle depth alone gains little.
  - Raising the path limit to 4,096 grows the handle-path cache (`fh_cache.c`, 4,096 entries) from 4 MiB to 16 MiB. The server's memory cap is 32 MiB (`vesper-microvm:94`).
- **ARC-3** Two possible fourth patches:
  - **(a) Small, low risk.** Drop `pwhash` and `gen` from the handle, giving 8 more levels (48).
    - `pwhash` is only checked for password exports (`nfs.c:69`), which Vesper does not use.
    - `gen` comes from `EXT2_IOC_GETVERSION`. On virtio-fs this probably always returns 0, but that is **Unknown**: check it by logging `gen` in the unfs suite.
    - Patch 0002's length checks would need to change too.
  - **(b) Larger, medium risk.** A "truncated" flag bit in `len`. When the handle is full, `fh_rec` searches the whole subtree below the last hashed directory for the (dev, ino) pair.
    - This search runs on every cache miss: after a server respawn (the supervisor allows up to 30 a minute), after LRU eviction (4,096 entries), and after a rename.
    - unfs3 is single-threaded, so one search over a large tree via virtio-fs `lstat` calls stalls all of that microVM's shares. It needs a cap on entries visited, returning STALE beyond it.
- **Next steps:** record ARC-2 in TD-067. Do (a) only if users ask for more depth. Reject (b) for now.
- **Test (if (a) is done):** the binds suite's `deep_path_and_rename` step expects depth 48. Add a step with a path near 1,024 bytes to measure ARC-2.

## 2. TD-069 (no I/O bound per share server): use the share network, not `io.max`

- **ARC-4** [S] `io.max` and `io.weight` act on block-layer I/O to a block device (major:minor). virtio-fs I/O goes through FUSE to the virtqueue and never reaches a block device, so neither control applies. The tracker row's premise ("a scheduler the System VM disk does not use") points at the wrong layer and should be corrected.
- **ARC-5** Practical alternative: Firecracker's own rate limiter on the `share0` interface (`vesper-microvm:623`, the network-interfaces config).
  - Its `tx_rate_limiter` (guest to host) can cap both bandwidth and operations per second; NFS requests and writes travel this way. Its `rx_rate_limiter` caps read replies.
  - Since every disk operation of the server starts from a guest request, this bounds its virtio-fs I/O. Together with the existing one-CPU `cpu.max`, that covers the noisy-neighbour case.
  - Because the shares are hard NFS mounts, excess demand stalls in the guest; it does not turn into errors.
  - The values need measuring (**Unknown**). Start high, for example 400 MB/s and 20,000 operations per second.
- **Recommendation:** implement now; it is one JSON field plus constants.
- **Test:** a microvm-shares suite step with a `dd` throughput run and a metadata storm (many `stat` calls), checking the bound and that a second microVM's latency holds. Run the unfs suite as a regression check.

## 3. GH #207 (binds from two shares): implement, with two fixes to the sketch

- **ARC-6** [S] Changes needed:
  - `config.go:30` `Source`: add `Mount int`. The dedup map in `MakePlan` (`shares.go:186`) then keys on it automatically.
  - `shares.go:120` `Plan.Mount` becomes `Mounts []string`. In `MakePlan` (`shares.go:184-234`), remove the refusal at 200-201 and add a mount index. `LaunchArgs` (`shares.go:237-250`) needs updating.
  - `sandbox_linux.go:24`: `Config.Mounts`. `OpenSources` (`sandbox_linux.go:68-125`) needs one mount fd per mount, each checked with `fstatfs`.
  - `cmd/vesper-unfs/main.go:96,110`: make `--mount` repeatable.
  - `vesper-microvm`:
    - Argument parsing (560-563) and the checks (585-586).
    - `unfs_start`: the `.args` write (418) and the `--mount` arguments (426).
    - The self-check reader (496-502).
  - `fanout.go`: `Server.Mounts` (33-37), per-source prefix matching in `Map` (107-133), `ReadServers` (186-210), and the mtime lookup in `Offer` (280).
  - No guest change is needed (`guest_linux.go` only sees export names).
- **ARC-7** [S] `.args` format. Today it is: mount, then squash, then `REL:mode` lines. Proposed:
  - First line `vesper-unfs-args 2`, then a mount count K, then K mount lines, then squash, then `IDX:REL:mode` lines.
  - Split `IDX` off at the first `:` (`ParseSource` already splits mode off at the last `:`).
  - The CLI argument is `--unfs-src IDX:REL:mode`.
  - The header lets the fanout detect old records during a shim upgrade.
- **ARC-8 (trap)** [S] The issue says "Nests never cross mounts, so the nest logic does not change". That is wrong as the code stands:
  - `Nests` (`config.go:103-126`) compares `Rel` strings only, so `a` (share 1, rw) and `a/b` (share 2, ro) would be treated as a nest. The sandbox's dev/ino check (`sandbox_linux.go:388-392`) would then refuse the server: a false failure.
  - Fix: `within` must require the same `Mount`.
- **ARC-9 (trap, security)** [S] If two virtio-fs shares overlap on the Mac (for example `/Users` and `/Users/x/proj`), a read-only bind from the inner share sitting inside a read-write source of the outer share is never nested.
  - The two are different mounts, and `open_tree` without `AT_RECURSIVE` (`sandbox_linux.go:197`) does not carry submounts.
  - The guest can therefore write the "read-only" directory through the outer export.
  - Fix: refuse a plan whose share mounts are nested (one path a prefix of the other). Whether the System VM paths always mirror the Mac paths is **Unknown**; check `vesper-share mount TAG PATH` usage. Record the rule in SECURITY.md.
- **Tests:**
  - Unit tests: `MakePlan` with two mounts, cross-mount `Nests` producing no nest, refusal of overlapping mounts, and an `.args` round trip in `fanout_test.go`.
  - Suites: unfs-suite self-check with two `--mount`; binds-suite steps for two shares, an event replayed from each, and a read-only source on share 2 that stays read-only.

## 4. GH #208 (single-file bind sources): implement after #207, with these additions

- **ARC-10** [S] The proposed design holds up.
  - `/s/N` is a tmpfs directory that is read-only after the sandbox's "read-only root" step (`sandbox_linux.go:424`). Guests therefore cannot create siblings, and `rename`/`unlink` of the file gives EBUSY (it is a mount point), which matches runc behaviour.
  - Files must use a fixed entry name such as `/s/N/f`, so the Mac file name never appears in the export.
- **ARC-11** [S] Code that needs a file variant:
  - Everything that assumes a directory today: `openLive` (`O_DIRECTORY`, line 138), `dirAlive` (183), `attachFd` (465-466), and the nest open (366-371).
  - `Init` must create an empty `f` before `move_mount`.
  - `Source` gets `File bool`, written as mode `rof`/`rwf` in `.args`.
  - Guest side: the `Share` sent to the guest gets `File`. Today `openInRoot` makes directories (`guest_linux.go:51-86`) and `mountOne` mounts NFS straight onto the destination (`guest_linux.go:134-160`). For a file, the guest mounts NFS at a hidden path outside `/newroot`, uses `open_tree` on `hidden/f`, and moves it onto an empty file created at the destination. Unmount order: the bind first, then the NFS mount.
- **ARC-12** [S] Two Unknowns about unfs3 itself:
  - The export root (tmpfs) and `f` (virtio-fs) report different `st_dev`, so the guest sees two different fsids within one export. The Linux NFSv3 client should be fine since `f` is not a directory, but this must be tested.
  - The handle's hash chain becomes `/s`, `/s/N`, `f`, so depth is not a concern.
- **ARC-13 (trap)** [S] Events are dropped today: `Map` skips `rest == ""` (`fanout.go:128`), so edits to a single-file source never reach the guest.
  - Fix: a file source emits `Rest="f"`. `Replay` touches `hidden/f` (`guest_linux.go:266-275`); inotify watches on the bound file fire because it is the same inode.
- **ARC-14 (trap)** [S] Rename-replace on the Mac (atomic saves): the cloned mount keeps the old inode. A later server respawn reopens the source by path and gets the new inode, so the guest's old handle goes STALE until the task restarts.
  - Characterise and document this, as Docker Desktop has the same limit for file binds.
- **ARC-15** [S] Nests: a read-only file inside a read-write directory source needs a file nest (`within` already works for this; the nest open must drop `O_DIRECTORY`). A read-write file inside a read-only directory needs no nest.
- **Tests:**
  - Unit tests: `MakePlan` file sources, `Nests` for a file, `Map` with `Rest="f"`.
  - unfs-suite: self-check that the export lists only `f`, and that a sibling create is refused (EROFS).
  - binds-suite:
    - Read and write the file.
    - A host edit triggers inotify in the guest.
    - A read-only file inside a read-write directory stays read-only.
    - `mv` over the file from the Mac, before and after `test-unfs-kill`, records the result.

All findings are from code reading; nothing ran in the System VM. The unfs3 source I read is in `/private/tmp/claude-501/-Volumes-Workspace-github-com-cityhunteur-vesper/3347cb86-4f04-4eee-abd6-5fcb4ee67780/scratchpad/council-arch/src/unfs3-0.10.0/`.
I reviewed this by reading the code and the unfs3 0.10.0 source; I ran no VMs. I downloaded the pinned tarball to the scratchpad (sha256 matches `third_party/unfs3/sources.sha256`). `go test` for `./internal/{unfs,shares,nfsprobe}` passes, but the results were cached. Everything below is static inspection unless it says executed.

### Baseline: what actually confines a handle today (static)
- **SEC-1.** A handle's 8-bit path hashes are not what keeps a guest inside its exports.
  - `fh_decomp` (`fh_cache.c:217`) first looks up `(dev, ino)` in a cache and does not check the hashes at all. Only then does it run `fh_rec`, which searches from `/` (the sandbox root, so `/usr`, `/lib` and `/etc` too).
  - The real guard runs after resolution: the `nfs.c:60` macro calls `exports_options(path)`, which matches the longest path prefix and refuses `/../`.
  - `locate_file`, the brute-force search, only runs with `-b`, and `UnfsdArgs` does not pass it.
  - So any change to the handle scheme must keep this rule: **the resolved path must be one a LOOKUP inside an export can reach, and the path check decides access.**
  - Side note: `exports_options` matches by prefix with no `/` boundary (`strstr(path, export)==path`), so `/s/1` also matches `/s/10…`. This is harmless today, because the longest match wins and every export is for the same guest address. It must stay that way if export names change.

### 1. TD-067 (depth limit): implement, with conditions
- **SEC-2.** Recommended scheme: keep the first ~40 hashes, set a "truncated" flag, and resolve deeper objects by a bounded `(dev, ino)` search below the deepest hashed directory. Required invariants:
  1. **Search only under `/s/N`.** Never search from `/` and never across mounts (`st_dev` must match, as `locate_pfx` does). Never enable `-b`.
  2. **The path check still runs on the resolved path.** The cache, which ignores hashes, must stay behind that check.
  3. **Extend patch 0002 to every new field.** A bad flag, a `len` that disagrees with the flag, or `ino == 0` must return `invalid_fh`. No `assert`, and every bound is checked before the `memcpy`.
  4. **Put a budget on each search** (count of directory entries visited), and answer STALE when it runs out. A forged deep handle otherwise makes the single-threaded server scan the whole export, which is per-server denial of service. `cpu.max` bounds that today, but nothing ends a single scan.
  5. **Inode guessing is not an escape** if invariants 1 and 2 hold: a guessed inode can only resolve to something already visible in an export. A guessed inode can reach a host-recycled inode, but only inside exports already granted, so that is acceptable. Record it.
- **Tests to add to `battery.go`:**
  - `forged` on a deep handle with the flag set and a wrong `ino` → STALE.
  - The flag set with `len` < max → STALE, and the server stays up.
  - A deep handle whose `ino` names `/s/1/...`, reached through `/s/0` hashes → it must resolve only under `/s/1`, with `/s/1`'s options. In particular, a read-only (ro) object must not become writable.
  - A deep handle for `/usr/sbin/unfsd`'s `(dev, ino)` (reported by the selfcheck) → STALE.
  - A 10k-entry tree with a missing `ino` → the reply is timed and the next request is still answered within N seconds.

### 2. TD-069 (no I/O bound): defer, but fix the record
- **SEC-3.** The tracker's reasoning is wrong. The traffic is NFS → unfsd → virtio-fs (FUSE to macOS), and no block device in the System VM is in that path. So `io.max` and `io.weight` would not apply even with a scheduler.
- What already bounds a server:
  - unfsd runs one select loop in one process (`daemon.c:969`), so it has at most one virtio-fs request in flight.
  - `cpu.max` is one CPU, and `memory.max` covers page cache.
- What is still exposed: a guest can still saturate macOS-side virtio-fs and APFS for the other microVMs on the same share. I rate this material only for multi-tenant use; it is local and not an escape.
- **Mechanism available:** a token-bucket filter (`tc` `tbf`/`police`) on the share TAP inside the microVM's network namespace bounds NFS bytes per second. Whether the System VM kernel has `CONFIG_NET_SCH_TBF` and `CONFIG_NET_ACT_POLICE` is **Unknown**; check its config. A lower `cpu.max` is the other knob.
- Rewrite TD-069 to say this.

### 3. GH #207 (two shares per server): implement, after two fixes
- **SEC-4 (high, static).** In `shares.go` MakePlan, the deduplication map is keyed on `unfs.Source{Rel, ReadOnly}`. If you add a second mount without adding the mount index to that key, `/A/x` and `/B/x` collapse into one export. The guest then gets share A's data at share B's destination, and its writes go to the wrong host tree.
  - Invariant: the source identity is `(mount index, Rel, ro)` everywhere: the planner, `OpenSources` (each source opened relative to its own mount's fd), `Nests`, the `.args` record and the fanout.
- **SEC-5 (high, static).** `Nests` compares `Rel` strings only.
  - Across mounts, a false nest fails closed: the sandbox's dev/ino check refuses it, so the server just doesn't start.
  - A **missed** nest is the real hole. `shared_directories_from_specs` (`src/systemvm.rs:77`) does not refuse overlapping macOS shares (for example `/Users/me` and `/Users/me/proj`). A ro bind through the inner share would stay writable through a rw bind of its parent through the outer share.
  - Fix, at least one of:
    - refuse overlapping share host paths when shares are configured; or
    - have the planner refuse a ro source and a rw source from different mounts when their host paths overlap.
- No change is needed to seccomp, `open_tree` or the nest attachment: each clone is a separate detached tree.
- **SEC-6.** `.args` is currently positional (line 1 is the mount). With several mounts it needs a format with explicit mount indexes, and the selfcheck should parse it with the same Go parser rather than the shell loop at `vesper-microvm:498`. Otherwise the escape tests can check a different sandbox from the one actually running.
- Each mount needs `unfs_mount_ok`.
- **Tests:**
  - Planner: the same `Rel` in two shares gives two exports.
  - Battery: two shares hold the same `Rel` with different content; read both and compare. LINK and RENAME from `/s/0` (share A) into `/s/1` (share B) → EXDEV.
  - Overlapping shares with mixed ro and rw → refused.
  - The selfcheck run from the multi-mount `.args`.

### 4. GH #208 (single-file binds): implement, with conditions
- **SEC-7.** `/s/N` becomes a directory in the read-only root tmpfs, holding one bind-mounted file. Required invariants:
  - **Fixed entry name.** The name inside `/s/N` is fixed and generated (for example `f`), never taken from the host file name. This matches how `Exports` keeps host paths out of the server.
  - **Source opening.** Open with `openat2` using `O_PATH` without `O_DIRECTORY`, plus `RESOLVE_NO_SYMLINKS|NO_XDEV|BENEATH`. That refuses a symbolic link at the source, including one swapped in after planning.
  - **Regular files only.** `fstat` must show `S_ISREG`. A FIFO or socket source would hang the single-threaded server on READ.
  - **`/s/N` itself must be read-only.** A root server holds `CAP_FOWNER` and `CAP_CHOWN`, so the read-only tmpfs is the only barrier. CREATE, REMOVE, RENAME, LINK, MKNOD and SETATTR (mode, uid, times) on `/s/N` must fail. This is the same lesson as the `/dev/null` fix: an inode that isn't on a share must not be changeable.
  - **Siblings are not exposed.** Only the file's mount sits in `/s/N`. `..` from `/s/N` reaches the tmpfs `/s`, and the existing `refusedHandle` check covers that.
  - **Rename-replace and hard links (accept and document).** A bind pins the inode, not the path. After a rename-replace on macOS the guest keeps the old inode. If the host moves the file elsewhere, the guest keeps access to it at the new place, the same as the directory nest note. Hard links share writes, as runc does. The guest cannot create a new link out of `/s/N`: the root is read-only, and links across mounts fail with EXDEV.
  - **Setattr on the file.** Truncate and chmod are allowed when the bind is rw and refused when ro. The file mount needs `nosuid`, `nodev` and `noexec`.
  - **MOUNT.** unfs3 MOUNT requires a directory (`mount.c:250`), so the guest must mount `/s/N`, never `/s/N/f`. The guest-side bind is a correctness question, not a host-security one.
- **Tests (battery):**
  - READDIR of `/s/N` returns only `f`.
  - All the mutations above on `/s/N` are refused with an NFS status, not a dropped connection.
  - Chmod through `/s/N/f` is refused when the bind is ro.
  - LINK and RENAME of `f` into a rw `/s/M` → EXDEV.
  - Mounting `/s/N/f` → NOTDIR.
  - Selfcheck: the source swapped for a symbolic link, or for a FIFO, between plan and open → the server is refused.
  - Suite: rename-replace on macOS, then read through the guest. Record whether it shows old data or ESTALE.
I recommend deferring #208 and TD-067, implementing #207 only after fixing three things its sketch misses, and correcting the TD-069 row. "Static" means I read the code; "executed" means I ran it.

Executed: `go test ./internal/shares/ ./internal/unfs/` passes (cached result). I fetched unfs3 0.10.0 into `/private/tmp/claude-501/-Volumes-Workspace-github-com-cityhunteur-vesper/3347cb86-4f04-4eee-abd6-5fcb4ee67780/scratchpad/council-cor/unfs3-0.10.0`; its SHA-256 matches `sources.sha256`. Nothing was built and no VM was started.

### TD-067: depth cap (defer; if it is done, use COR-3)

**COR-1 (static): the tracker row describes the handle wrongly.** The handle does not hold the path. It holds one 8-bit hash of the inode number per path component (`fh.c` FH_HASH, `fh_comp_raw`, `fh_extend` lines 269-300). Decoding (`fh_rec`, lines 375-450) walks down from the root and goes into every entry whose inode hash matches.
- The 64-byte limit comes from NFSv3 itself (NFS3_FHSIZE), so making the handle bigger is not an option.
- `fh_extend` returns NULL at `len == FH_MAXLEN` (43), and LOOKUP then answers NAMETOOLONG (`nfs.c:224`). `/s/N` takes 2 of the 43 slots.
- Correct the TD-067 wording in `docs/exec-plans/tech-debt-tracker.md:82`.

**COR-2 (static): rename behaviour today.**
- Renaming a directory within the same parent keeps every handle valid, because the hashes come from inode numbers, not names.
- Moving it to another parent makes handles below it stale. Open files and the working directory get ESTALE; a new walk by path recovers.
- Handles carry no server state, so they survive a server restart. `fh_cache` (4096 LRU entries, held in memory) only speeds decoding up.

**COR-3 (static): the only workable fix.** It would be a fourth unfs3 patch. A full handle carries a marker; the generation field is always 0 on FUSE, so it can hold a flag. On decode the server tries `fh_cache` first, then searches the subtree below the 41-level prefix for an exact (dev, ino) match.
- **Risk:** after a TD-065 respawn the cache is empty. Every deep handle the guest still holds then needs an unbounded search under the one-CPU `cpu.max`, while the guest's hard mounts wait. Cap the search by entries visited, and answer ESTALE when the cap is reached.
- **Risk:** TD-071 says virtio-fs can report stale inode numbers, which gives false ESTALE answers, as today.
- **Must keep:** handles with `len < 43` must stay byte-identical, or `server_restarts_and_old_handles_measured` changes meaning. Patch 0002's bounds check must still hold.
- **Tests:**
  - An `nfsprobe` battery for a 60-level save and check across a kill.
  - A suite step: create a 60-level tree, open a file and set the working directory deep, kill unfsd, then read from the old handle.
  - A suite step: rename a deep directory in place and across parents.

**COR-4 (Unknown): `open_by_handle_at` as an alternative.** It needs CAP_DAC_READ_SEARCH, which `RootCaps` does not include (`sandbox_linux.go:58`), and it is unknown whether Apple's virtio-fs supports export operations. I reject it.

### TD-069: I/O bound (fix the tracker row)

**COR-5 (static): the row's premise is wrong.** The server's I/O goes through FUSE/virtio-fs requests. Those are not block-layer requests, so `io.max` and `io.weight` would not limit them whatever disk scheduler is used. The working limits are `cpu.max` and `memory.max`.
- A throttle inside unfsd would only slow replies. With hard mounts that means stalls, not errors.
- Recommendation: close the item as "not achievable with cgroups", or keep it as a request rate limit.

### GH #207: binds from two shares (implement, after these fixes)

**COR-6 (static): two places in the sketch would give wrong results.**
- **Wrong directory exported:** `MakePlan` de-duplicates on `map[unfs.Source]int` (`shares.go:186,221`). Two shares holding the same relative path and mode would collapse into one export. The mount index must be part of the key.
- **Server fails to start:** `Nests` and `within` (`config.go:88-126`) compare relative paths only. A read-only source in share B whose path lies under a read-write source in share A would get a nest. `Init`'s dev/inode check (`sandbox_linux.go:~380`) then refuses it and the server does not start. The issue's claim that the nest logic does not change is false: nesting must require the same mount.

**COR-7 (static): everything that must change.**
- `unfs.Source` gets a `Mount int` field.
- `Plan.Mount` becomes `Mounts []string`; remove the refusal at `shares.go:200`; update `LaunchArgs` (`shares.go:241`).
- `vesper-unfs run`: `--mount` becomes repeatable, with each `--src` belonging to the `--mount` before it (`main.go:96-114`).
- `OpenSources` checks and opens each mount once (`sandbox_linux.go:72-95`).
- `vesper-microvm`: argument parsing (`:560`), `unfs_mount_ok` for each mount (`:586`), `unfs_start` and the `.args` writer (`:396`, `:418`), and `test_unfs_selfcheck` (`:496-504`).
- Fanout: `Server.Mounts`, `ReadServers` (`fanout.go:186-211`), and `Map`, which must take the longest matching mount as `shareOf` does.
- `Offer` calls `mtime(s.Mount, …)` (`fanout.go:280`); this must use the mount of each event.
- The guest side, `Exports`, `unfs_stop` and `StopShares` need no change.

**COR-8 (static): old and new versions side by side.**
- **Old `vesper-microvm` with a new shim:** a second `--unfs-mount` silently replaces the first (`:560`), so share A's sources would be opened under share B. They ship together in the overlay and `/run` is tmpfs, so this mix should not happen in practice. Even so, the new shell should refuse an `--unfs-src` that comes before any `--unfs-mount`.
- **New `.args` format:** start it with a line-0 sentinel such as `#v2`. An old fanout then reads a mount that is not absolute, `Map` returns nothing, and it fails closed. New readers must still accept v1 (line 0 absolute).
- Whether binaries are swapped live during the development loop is Unknown; check DEVELOPMENT.md.

**COR-9 (static): a latent fanout bug, fix it with #207.** `ReadServers` skips a source line it cannot parse (`fanout.go:202-205`). Every later source then gets the wrong index, events go to the wrong export, and `touchInRoot` can push an unrelated file's mtime forward. A source line that fails to parse should drop the whole record. Also, `CheckRel` accepts a newline, which the shell then rejects late; `MakePlan` should reject it early.

**Tests for #207:**
- `MakePlan` cases: same relative path in two shares; a read-only path in B under a read-write path in A gives no nest.
- `ReadServers` v1/v2 round trip, and the bad-line case above.
- `Map` with nested share mounts.
- A suite step with two shares and events from both.

### GH #208: single-file binds (defer; prototype first)

**COR-10 (static): unknowns that block the proposed design.**
- **Mount crossing:** the export root would be the sandbox's tmpfs while the file is on virtio-fs. unfs3 reports `fsid = st_dev` (`attr.c:180`), so the guest's NFS client sees an fsid change at the file. Whether Linux NFSv3 treats that as a mount point or refuses it is Unknown.
- **Writable tmpfs:** the tmpfs (`sandbox_linux.go:327`, 1 MB) would be writable through a read-write export, letting the guest create sibling files. Use a read-only directory for `/s/N`.

**COR-11 (static): code that assumes directories.**
- `MakePlan` must refuse anything that is not a regular file (sockets such as `docker.sock`, FIFOs, devices).
- Nest lookup at `sandbox_linux.go:371`, `openInRoot`, and `unmountList` (`guest_linux.go:206`) all use `O_DIRECTORY`.
- The guest needs to create a missing file for the destination and refuse a destination that is a directory.
- A read-only bind needs `MS_BIND` and then a remount with read-only (`MS_REMOUNT|MS_BIND|MS_RDONLY`), plus `nosuid,nodev`.
- A nest of a read-only file inside a read-write directory must open without `O_DIRECTORY` and compare the file type.

**COR-12 (static): no events reach a file bind.** For a file source the event path equals the source's relative path, so `rest == ""`, and `Map` drops it (`fanout.go:128`). It needs a file flag that sends `Rest: ""`; the guest then touches the destination itself. `ParseEvent` already accepts an empty rest. Read-only file binds get no events, as with read-only directories.

**COR-13 (Unknown, needs a test): rename-replace on macOS.** The clone pins the old FUSE node. Three outcomes are possible:
- The guest keeps seeing the old content, as Docker does on Linux.
- Permanent ESTALE: `attr.c:335/422` compare the handle's inode with `lstat`, and the guest's bind holds that dentry.
- If Apple's virtio-fs answers by name (as TD-071 suggests), new data read with the old cached size, giving truncated or padded reads.

The suite step should save a file on macOS by rename and record the guest's content, size and errno.

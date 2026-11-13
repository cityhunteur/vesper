## Code Review Council Report

**🚧 Merge after fixes** · 0 blockers
🔴 0 Critical · 🟠 0 High · 🟡 3 Medium · 🔵 6 Low · ▲ 1 coverage gap
`systemvm/overlay/usr/local/bin/vesper-vesperd` +13 more (14 files, +305 −30) · Rust, POSIX sh (busybox)
Agents: Codex · Claude · Antigravity · Judge

Two things need fixing. The guest helper's `ip link set` call reads some valid tap names as keywords. The owning docs and the published replay report also still describe the old behaviour. The judge confirmed all 12 reviewer findings: none is Critical or High. The tap and namespace flow from the CLI to the jailer is sound, the targeted Rust tests pass and no dependencies changed.

### Next steps
1. **Close the gap** ▲ Security → rerun the Codex seat against a live System VM. Show that a jailed Firecracker cannot reach the root namespace or another lease's namespace.
2. **Fix #2 in this change.** AGENTS.md invariant 7 requires the docs in the same change → `grep -n "18 steps" ARCHITECTURE.md docs/process/systemvm-bringup.md`
3. **Fix #1** before the next System VM run → `cargo test --lib tap_names` (after you add the `up` assertion)
4. **Follow up** after merge: #3, #4, #5, #6, #7, #8, #9

### 🟡 Medium

**#1 · Tap names that are `ip link` keywords pass validation, then fail the launch**
`systemvm/overlay/usr/local/bin/vesper-vesperd:207` · Correctness · Claude F1, Antigravity F1 · ✓ verified

Line 207 runs `ip link set "$t" up` without `dev`. Both `valid_tap` (lines 98-101) and `valid_tap_name` (`src/control/launchapi.rs:39-44`) accept names such as `up`, `mtu` and `dev`. The launch then fails after `tunctl` has already created the tap, and the error does not say the name is the problem. It fails closed. Antigravity rated it High and the judge lowered it to Medium. Antigravity also says hyphen-led names (`-`, `-tap0`) are accepted. That is plausible, but no failure was shown.
- **Verify:** add `assert!(!valid_tap_name("up"))` to `tap_names_are_short_interface_names` and run `cargo test --lib tap_names`. It fails today. In a System VM, `vesperd --backend systemvm --id x --tap up` returns `ERR network: tap up up: …`.
- **Fix:** use `ip link set dev "$t" up` at line 207 and `ip link set dev lo up` at line 204. You can also reject a leading `-` in all three validators.

**#2 · Owning docs and parity artifacts contradict the code (invariant 7)**
`docs/specs/compat-exceptions.yaml:145-162` +6 more · Design and maintainability · Claude F2 · ✓ verified

- CE-0010 is still `status: open` and still says "14 of the 173 cases".
- `docs/generated/backend-a-behavior-replay.json` is stamped `bdcb467` and shows 156 matched, 2 mismatched and 12 setup failures. It holds 19 `Open tap device failed` entries.
- `ARCHITECTURE.md:245`, `systemvm-bringup.md:547` and `src/README.md:456` say 18 steps, but the code requires 19.
- `src/bin/README.md:42` and `systemvm-bringup.md:501` still give the old `LAUNCHAPI` grammar.
- The usage header at `vesper-vesperd:35-36` does not list `netns` and gives an incomplete description of `isolation`.
- **Verify:** `grep -n "18 steps" ARCHITECTURE.md docs/process/systemvm-bringup.md` and `grep -n -A3 '"summary"' docs/generated/backend-a-behavior-replay.json`
- **Fix:** rerun and publish the System VM replay suite, then update or close CE-0010 with the new counts. Correct the step counts, the grammar lines and the script's usage header.

**#3 · No test covers concurrent namespace isolation, the shell tap logic or the CE-0010 replay**
`src/control/suites/vesperd.rs:406` · Testing · Antigravity F7 · ✓ verified (judge: Low-Medium)

- `tap_devices_in_own_netns` starts only one microVM and compares its namespace only with pid 1's. A bug that put every microVM in one shared non-root namespace would still pass.
- No hermetic test runs the shell tap and netns code.
- `src/bin/systemvmctl/replay.rs` fails only on `backend_error`. It exits 0 when cases are mismatched or fail setup.
- **Verify:** `grep -c "Open tap device failed" docs/generated/backend-a-behavior-replay.json` (returns 19)
- **Fix:**
  - Start a second concurrent `--tap tap0` microVM in `tap_devices_in_own_netns`. Assert `LINK lo state=UP`, and assert that `net` fails after teardown.
  - Add a shell self-test for tap validation.
  - Make `systemvmctl replay` fail when `mismatched > 0` or `setup_failed > 0`.

### 🔵 Low

- **#4** `src/bin/vesperd/stage.rs:35` · API and compatibility · Claude F3 · ✓ verified — the guard for the `stage` and `store` subcommands has no `args.tap` term, so `vesperd --tap tap0 store list` ignores the flag → add `|| !args.tap.is_empty()`, name `--tap` in the message, and add an assertion to `bad_stage_and_store_arguments_never_reach_the_agent`.
- **#5** `src/api_replay.rs:152-207` · Correctness · Claude F4, Antigravity F6 · ✓ verified (Antigravity: Medium) — `validate_replay_scripts` does not check `snapshot_fixture` steps or nested `network_overrides[].host_dev_name`, and does not enforce `MAX_TAPS = 8` → run `host_dev_names` over `expand_fixture`, walk the JSON recursively and check `taps.len() <= 8`.
- **#6** `vesper-vesperd:635,714` · Correctness · Claude F5, Antigravity F3 · ✓ verified (Antigravity: Medium) — `netns_ok` never compares Firecracker's namespace with the one at `$NETNS/$sid`. Both `netns_ok` and `separate` also report true when `readlink` or `nsenter` returns nothing → compare inodes (`stat -L -c %i`) and require non-empty values.
- **#7** `vesper-vesperd:195,536` · Concurrency · Antigravity F2 · ✓ verified (Antigravity: High) — `net()` runs without `$LOCK`, `umount` without `-l` can hit `EBUSY`, and the busy check at line 536 leaves out `$NETNS/$LID`. The race window is small → use `umount -l` and base the `ERR busy` check on `leftovers`.
- **#8** `vesper-vesperd:559,711` · Scalability and performance · Antigravity F4 · ✓ verified (Antigravity: Medium) — `netns_up` runs under the global lock, a launch with only `tap=` tokens still unlocks and relocks, and `net()` mounts sysfs once per interface. The judge estimates a few milliseconds → mount sysfs once per `net` call and consider running `netns_up` outside the lock.
- **#9** `vesper-vesperd:240-246,640-648` · Observability · Antigravity F5 · ✓ verified (Antigravity: Medium) — `list()` leaves out `$NETNS`, so `nothing_left` misses a leaked namespace, and the `reap_pass` netns loop prints nothing. The `teardown > /dev/null` at line 498 was already there → print `NETNS` lines in `list` and `REAPED … reason=orphan-netns` from `reap_pass`.

### Coverage
A gap means nobody reviewed the lens, not that it is fine.

| Lens | Status | Evidence |
| --- | --- | --- |
| Security | ▲ gap | Codex "not checked"; no live isolation run |
| Privacy | covered | Codex: no new personal data (thin) |
| Supply chain | covered | Codex: no dependency or CI changes |
| Correctness | covered | #1, #5, #6; Claude traced CLI to jailer |
| API and compatibility | covered | #4 |
| Rollout and migration | covered | Claude: old/new guest mix, tmpfs reboot |
| Reliability | covered | #1, #5 |
| Scalability and performance | covered | #8 |
| Concurrency | covered | #6, #7 |
| Observability | covered | #9 |
| Testing | covered | #3 |
| Design and maintainability | covered | #2; tap rule exists in three copies |
| Readability and idioms | covered | Claude: one nit (`launch_stage` skips) |

### Consensus
| # | Codex | Claude | Antigravity | Judge |
| --- | --- | --- | --- | --- |
| 1 | ○ | ● | ● | ✓ |
| 2 | ○ | ● | ○ | ✓ |
| 3 | ○ | ○ | ● | ✓ |
| 4 | ○ | ● | ○ | ✓ |
| 5 | ○ | ● | ● | ✓ |
| 6 | ○ | ● | ● | ✓ |
| 7 | ○ | ○ | ● | ✓ |
| 8 | ○ | ○ | ● | ✓ |
| 9 | ○ | ○ | ● | ✓ |

Dropped as false: none.
● flagged · ○ not flagged · ✓ verified · ? unverified · ✗ false

### Strengths
- The guest tap validator rejected 20 hostile inputs and accepted 4 valid names (Codex).
- Targeted Rust suites pass: `vesperd_systemvm`, `api_replay` and `control::launchapi`.
- The rollout is safe both ways: an old guest returns a clean `ERR`, and a reboot clears the namespaces.

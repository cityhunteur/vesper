# Completed ExecPlans

Finished plans. Keep them: their Decision Logs and retrospectives explain why the code looks the way it does.

| Plan | Outcome | Completed |
|---|---|---|
| [2026-11-06-WOR-24-nested-virtualization-gating.md](2026-11-06-WOR-24-nested-virtualization-gating.md) | Host capability probe, nested-virtualization policy, Backend A/B selection with diagnostics; verified on an M3 and on CI's M2 runner (GH #17 closed) | 2026-11-06 |
| [2026-11-06-WOR-21-systemvm-reliability-suite.md](2026-11-06-WOR-21-systemvm-reliability-suite.md) | N-cycle boot stress, socket smoke, leak checks and a JSON health report; first run caught threshold overruns, fixed by detaching the daemon start-up (GH #16 closed) | 2026-11-06 |
| [2026-11-06-WOR-23-docker-socket-ingress.md](2026-11-06-WOR-23-docker-socket-ingress.md) | Docker CLI on macOS reaches the guest's Docker through vesper-proxy; trivial container runs; raw sockets private (GH #15 closed) | 2026-11-06 |
| [2026-11-06-WOR-33-systemvm-runtime-services.md](2026-11-06-WOR-33-systemvm-runtime-services.md) | containerd and dockerd supervised in the guest, crash recovery under 600 ms proven from the host (GH #14 closed) | 2026-11-06 |
| [2026-11-06-WOR-25-systemvm-control-channel.md](2026-11-06-WOR-25-systemvm-control-channel.md) | Pinned guest packages via an initramfs overlay; launcher vsock bridge; supervised guest agent; control client with handshake, keepalive, reconnect and 2 MiB echo proven (GH #13 closed) | 2026-11-06 |
| [2026-11-06-WOR-27-systemvm-image-pipeline.md](2026-11-06-WOR-27-systemvm-image-pipeline.md) | Pinned, verified System VM image; Swift launcher; 200/200 probed boots with NAT proof under 5 s (GH #12 closed) | 2026-11-06 |

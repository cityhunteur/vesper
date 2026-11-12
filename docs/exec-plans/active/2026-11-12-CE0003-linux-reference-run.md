# Linux aarch64 KVM reference run of the upstream Firecracker suite (D-023, D-024)

This ExecPlan is a living document. The sections Progress, Surprises & Discoveries, Decision Log, and
Outcomes & Retrospective must be kept up to date as work proceeds. It is maintained in accordance with
docs/PLANS.md.

Trackers: GitHub #172 (comparator), history in GitHub #166. Decisions D-014, D-023, D-024. Exceptions CE-0001,
CE-0003.

**Gate: this plan runs once, when Vesper is code complete (D-024), and not before.** Code complete means ADR 0007
slices 1 to 6 are merged (GH #156) and the scripted Backend A run with its comparator is merged (GH #172), so one
paid run produces the pass set and the comparison together. A paid host also needs, at that time, the owner's
approval of a current quote and of a CPU quota increase.

## Purpose / Big Picture

D-023 says the upstream-suite pass set comes from stock Firecracker on Linux aarch64 KVM at the locked revision, not
from Backend A. The GH #166 trial ran the suite in the System VM (755 of 980 passed) and left 56 unexplained
failures that only a Linux run can sort into "fails upstream too" or "Backend A differs". After this plan, the
repository holds a per-test `{nodeid: outcome}` list from Linux and a comparison with Backend A, each citing
`d83d72b710361a10294480131377b1b00b163af8`. Passing on Linux alone is not Vesper parity.

## Progress

- [x] (2026-11-12) Host options checked against Google's documentation; Free Tier blocker recorded; region and
      timing chosen by the owner (D-024).
- [ ] Code-complete gate met (ADR 0007 slices 1 to 6, GH #172).
- [ ] Owner approves the quote and the quota increase; read-only checks in step A done.
- [ ] Host created, checked (step D), suites run, evidence retrieved, host deleted.
- [ ] Comparison written; CE-0001, CE-0003, D-023 consequences and GH #172 updated.

## Surprises & Discoveries

- Observation: no Google Cloud Free Tier machine can do this. Always Free covers one `e2-micro` (x86) a month in
  three US regions; nested virtualisation excludes "E2 VMs" and "VMs powered by ARM processors", so no Arm VM gets
  `/dev/kvm`. The only Arm machine with KVM is C4A bare metal, 96 vCPUs, paid.
  Evidence: docs.cloud.google.com `free/docs/free-cloud-features`, `compute/docs/instances/nested-virtualization/overview`,
  `compute/docs/instances/bare-metal-instances`, read 2026-11-12.
- Observation: C4A bare metal is not offered in europe-west2 (London) by Google's zone table; europe-west4 a/b/c and
  europe-west3 a/b/c are the European zones. Third-party listings disagree; `machine-types list` in the project settles it.
- Observation: the project's `CPUS_ALL_REGIONS` quota was 32; the host needs 96.
- Observation: Google's runtime-limit page does not say whether `--max-run-duration` applies to bare metal.

## Decision Log

- Decision: europe-west4, `c4a-highmem-96-metal`, on-demand (not Spot, which the docs do not confirm for bare
  metal), one run at code complete. Date/Author: 2026-11-12, owner (D-024).

## Outcomes & Retrospective

Not started.

## Context and Orientation

- Pinned revision: [design-v0.baselines.lock.yaml](../../specs/design-v0.baselines.lock.yaml) (Firecracker v1.16.0,
  `d83d72b`). Re-read it at run time; if it changed, re-pin everything below first.
- Prior run: [GH #166 plan](../completed/2026-11-12-GH166-upstream-suite-trial.md), failure classes and the harness
  deviations. Its local helpers in `run/GH-166/` hold macOS paths and System VM workarounds and must not run on Linux.
- Upstream: `tools/devtool`, `tests/README.md`, `tests/pytest.ini`, `.buildkite/pipeline_release_qa.py` at `d83d72b`.
- Cloud: project `pg-lab-1`; Compute calls by short-lived impersonation of
  `vesper-parity@pg-lab-1.iam.gserviceaccount.com` (role `roles/compute.instanceAdmin.v1`, project-wide) from Cloud
  Shell under the owner's personal account. No keys, no API keys, never the local work identity. Budget
  `4ff2f590-05cd-407a-b9f2-0bc0774c4fc0` (GBP 60, project filter, credits excluded, alerts 50/80/100 %) and the
  account budget stay; neither stops a machine.

## Plan of Work

A. Read-only checks. B. Quote and approval. C. Create one bounded host. D. Prove KVM. E. Pin the upstream
environment. F. Run smoke, collection, functional, security. G. Compare, retrieve, delete, record.

Cost estimate (2026-11-12, third-party price, to be replaced by a quote in step B): compute $5.9412/h in
europe-west4 × 4 h = $23.76; Hyperdisk Balanced about 250 GB for 4 h about $0.15; external IPv4 about $0.02;
egress about 5 GB about $0.60; about $24.50 before tax, about $29.50 with 20 % VAT. Each extra hour adds about $5.94.

## Concrete Steps

A. In Cloud Shell (personal account), all with `--project=pg-lab-1 --impersonate-service-account=vesper-parity@pg-lab-1.iam.gserviceaccount.com`:

    gcloud auth list --filter=status:ACTIVE --format='value(account)'
    gcloud compute machine-types list --filter='name~c4a.*metal' --zones=europe-west4-a,europe-west4-b,europe-west4-c
    gcloud alpha services quota list --service=compute.googleapis.com --consumer=projects/pg-lab-1 \
        --filter='metric~C4A OR metric~CPUS_ALL_REGIONS'

  and verify the budget. Image: an arm64 image with the IDPF driver that bare metal requires (check the OS details
  page; Ubuntu 24.04 LTS arm64 is the candidate).

B. Quote from the Pricing Calculator or the Cloud Billing catalogue: compute, Hyperdisk capacity and provisioned
  performance, IPv4, egress, tax; currency, region, source and arithmetic recorded here. No reservation or
  committed-use purchase.

C. Create only when D to G are scripted:

    gcloud compute instances create vesper-ref-<utc> --zone=europe-west4-a \
        --machine-type=c4a-highmem-96-metal --maintenance-policy=TERMINATE --provisioning-model=STANDARD \
        --max-run-duration=4h --instance-termination-action=DELETE --no-restart-on-failure \
        --no-service-account --no-scopes \
        --create-disk=boot=yes,auto-delete=yes,type=hyperdisk-balanced,size=250GB,image-family=<idpf-arm64>,image-project=<project>

  Then `instances describe --format='yaml(scheduling,disks)'`; if the deadline or auto-delete is missing, delete at
  once. SSH only (IAP or port 22 from the operator's address); Firecracker sockets stay on the host.

D. Record instance, zone, machine type, image, `uname -m` (must be `aarch64`), `lscpu`, kernel, memory, disk,
  deadline. KVM probe: open `/dev/kvm`, `KVM_GET_API_VERSION` returns 12, `KVM_CREATE_VM` returns a descriptor.
  Check `/dev/net/tun`, cgroup v2, `CONFIG_USERFAULTFD`. On failure collect diagnostics and delete the host.

E. Shallow checkout at the locked SHA with a clean `git status`; `public.ecr.aws/firecracker/fcuvm:v90` with its
  digest; artefacts pinned to `s3://spec.ccfc.min/firecracker-ci/20260603-e656faf58d6c-0/aarch64`; v1.16.0 release
  binaries (sha256 as in `systemvm/archives.yaml`) plus the examples, `.debug` files and instrumented helpers
  (`clippy-tracing`, `log-instrument`) that `pipeline_release_qa.py` builds. Prefer unmodified `tools/devtool test`.

F. Smoke `integration_tests/functional/test_api.py`; `--collect-only -q integration_tests/functional
  integration_tests/security`; then functional and security with `-n 3 --dist worksteal --timeout=300
  --timeout-method=thread`, CPU affinity recorded, under an outer `timeout 3h`. Keep JUnit XML, pytest-json-report,
  `test_results/`, logs, command lines, exit codes and wall times; reruns in their own directory. No change to test
  bodies, thresholds or markers; no performance suite.

G. Copy evidence to `run/GH-166/linux-ref-<utc>/` and check it reads; delete the instance; verify no disk, address
  or firewall rule from the run remains. Keep the service account and budgets.

## Validation and Acceptance

Accepted when: the host check in D passed; the outcome list covers every collected node id; the comparison with
`run/GH-166/results/combined-classified.tsv` reports matched, added, missing, skipped and deselected tests
separately and sorts failures into reproduced on Linux, System VM only, missing dependency, CPU or kernel feature,
timing, and unresolved; every claim cites `d83d72b`; cleanup is verified; `python3 harness/check_docs.py` passes.

## Idempotence and Recovery

The host is disposable and deletes itself after 4 h. A failed run is repeated from step C with a new name; evidence
directories are timestamped and never overwritten.

## Artifacts and Notes

Evidence lands in `run/GH-166/linux-ref-<utc>/` (local, untracked); the outcome list and the comparison summary are
committed with GH #172.

## Interfaces and Dependencies

No product interface changes. The run uses Google Compute Engine, the upstream `fcuvm` image and the public
`spec.ccfc.min` bucket; none becomes a Vesper dependency.

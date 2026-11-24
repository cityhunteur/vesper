CARGO ?= cargo
RUN_DIR ?= run
CYCLES ?= 50

.PHONY: lint build systemvm-upstream-suite third-party-notices third-party-notices-check test contract-generate contract-check contract-test surface-matrix-generate \
	surface-matrix-check behavior-matrix-generate behavior-matrix-check \
	validate-entitlements dev-up dev-down vmlauncher-build systemvm-fetch systemvm-overlay systemvm-prepare systemvm-build systemvm-launch-config \
	systemvm-boot-smoke systemvm-up systemvm-down systemvm-control-suite systemvm-services-suite systemvm-trivial-image docker-up docker-down docker-smoke docker-conformance ports-suite binds-suite data-disk-suite share-notify-selftest share-watch-selftest systemvm-up-shares dns-suite workflow-suite crossarch-suite ctr-conformance ctr-conformance-check ctr-conformance-loop systemvm-reliability systemvm-microvm-suite systemvm-rootfs-suite systemvm-workload-suite systemvm-parity-baseline parity-baseline-check shim-build shim-test systemvm-exec-suite systemvm-shim-suite systemvm-bundle-suite systemvm-restart-suite systemvm-launchapi-suite systemvm-vesperd-suite systemvm-up-snapshot systemvm-snapshot-suite systemvm-replay-suite systemvm-replay-publish behavior-replay-check systemvm-egress-suite systemvm-bounds-suite systemvm-sharenet-suite systemvm-confine-suite systemvm-unfs-suite systemvm-microvm-shares-suite systemvm-microvm-binds-suite systemvm-microvm-size-suite systemvm-docker-microvm-suite launcher-config-contract sign nightly phase4-vesper docker-microvm-measure vesper-default-gate vesper-overcommit-check

build:
	$(CARGO) build --bins

# TD-006: formatting and clippy with the pinned toolchain; CI runs it in rust-tests.yml.
lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

test:
	$(CARGO) test

contract-generate:
	$(CARGO) run --bin contractgen -- --mode generate

contract-check:
	$(CARGO) run --bin contractgen -- --mode check

contract-test: contract-check
	$(CARGO) test --test api_contract --test contract_golden

surface-matrix-generate:
	$(CARGO) run --bin baselinectl -- --mode generate-api-matrix

surface-matrix-check:
	$(CARGO) run --bin baselinectl -- --mode validate-api-matrix

behavior-matrix-generate:
	$(CARGO) run --bin baselinectl -- --mode generate-api-behavior-matrix

behavior-matrix-check:
	$(CARGO) run --bin baselinectl -- --mode validate-api-behavior-matrix

validate-entitlements:
	bash scripts/ci/validate-entitlements.sh

dev-up:
	bash scripts/macos/dev-up.sh

dev-down:
	bash scripts/macos/dev-down.sh

# Host binaries: ad hoc, hardened runtime, their plists (ADR 0012).
sign:
	cargo build -q --bins
	bash scripts/macos/sign.sh debug

vmlauncher-build:
	bash scripts/macos/build-vmlauncher.sh

systemvm-fetch:
	bash scripts/systemvm/fetch-image.sh

systemvm-overlay:
	bash scripts/systemvm/fetch-packages.sh
	bash scripts/systemvm/build-overlay.sh

systemvm-prepare: systemvm-fetch systemvm-overlay systemvm-launch-config vmlauncher-build

systemvm-build:
	bash scripts/systemvm/build-image.sh

systemvm-launch-config:
	bash scripts/systemvm/generate-launch-config.sh

systemvm-boot-smoke:
	bash scripts/systemvm/boot-smoke.sh

systemvm-up:
	bash scripts/systemvm/up.sh

systemvm-down:
	bash scripts/systemvm/down.sh

systemvm-control-suite:
	$(CARGO) run --bin systemvmctl -- control suite

systemvm-services-suite:
	$(CARGO) run --bin systemvmctl -- control services-suite

systemvm-microvm-suite:
	$(CARGO) run --bin systemvmctl -- control microvm-suite

systemvm-sharenet-suite:
	$(CARGO) run --bin systemvmctl -- control sharenet-suite

systemvm-rootfs-suite:
	$(CARGO) run --bin systemvmctl -- control rootfs-suite

systemvm-workload-suite:
	$(CARGO) run --bin systemvmctl -- control workload-suite

systemvm-parity-baseline:
	bash scripts/systemvm/parity-baseline.sh $(PERF_CYCLES) $(PARITY_FLAGS)

shim-build:
	bash scripts/systemvm/build-shim.sh

shim-test:
	cd shim && go vet ./... && go test -count=1 ./...

systemvm-exec-suite:
	$(CARGO) run --bin systemvmctl -- control exec-suite

systemvm-shim-suite:
	$(CARGO) run --bin systemvmctl -- control shim-suite

systemvm-bundle-suite:
	$(CARGO) run --bin systemvmctl -- control bundle-suite

systemvm-restart-suite:
	$(CARGO) run --bin systemvmctl -- control restart-suite

systemvm-launchapi-suite:
	$(CARGO) run --bin systemvmctl -- control launchapi-suite

systemvm-vesperd-suite:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control vesperd-suite

# ADR 0007 slice 5: the snapshot suite stages files from run/vesperd-share, so the System VM must share it.
systemvm-up-snapshot:
	mkdir -p run/vesperd-share
	VESPER_SHARES="$(CURDIR)/run/vesperd-share" bash scripts/systemvm/generate-launch-config.sh
	bash scripts/systemvm/up.sh

# GH #172: the upstream Firecracker suite on Backend A (about 80 min; restarts the System VM with 4 vCPUs and 8 GiB,
# then restores it). UPSTREAM_ARGS selects tests for a quick run, e.g. integration_tests/functional/test_api.py.
systemvm-upstream-suite:
	bash scripts/systemvm/upstream-suite/run.sh $(UPSTREAM_ARGS)

systemvm-snapshot-suite:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control snapshot-suite

# ADR 0007 slice 6: replay the behaviour matrix through vesperd on Backend A, one microVM per case
# (needs make systemvm-up-snapshot). The publish target also rewrites the committed report.
systemvm-replay-suite:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control replay-suite

systemvm-replay-publish:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control replay-suite --publish docs/generated/backend-a-behavior-replay.json

# ADR 0008: vesperd --network nat guests probe their egress from inside (needs make systemvm-up and docker running).
systemvm-bounds-suite:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control bounds-suite

systemvm-egress-suite:
	$(CARGO) build --bin vesperd
	bash scripts/macos/sign.sh debug vesperd
	$(CARGO) run --bin systemvmctl -- control egress-suite

# The replay scripts cover the matrix exactly and reproduce it on the model backend (runs in CI with cargo test).
behavior-replay-check:
	$(CARGO) test --test api_replay

parity-baseline-check:
	python3 -I scripts/systemvm/parity_baseline_report.py --self-test
	python3 -I scripts/systemvm/parity_baseline_report.py --validate docs/specs/backend-a-parity-baseline.json

systemvm-trivial-image:
	bash scripts/systemvm/build-trivial-image.sh

docker-up:
	bash scripts/systemvm/docker-up.sh

docker-down:
	bash scripts/systemvm/docker-down.sh

docker-smoke:
	bash scripts/systemvm/docker-smoke.sh

docker-conformance:
	bash scripts/systemvm/docker-conformance.sh

ports-suite:
	bash scripts/systemvm/ports-suite.sh

# Regenerate the launch config with the three bind-mount test shares (rw watched, ro, rw :nowatch) and boot (GH #29, GH #150).
systemvm-up-shares:
	mkdir -p run/WOR-39/share run/WOR-39/share-ro run/WOR-39/share-nowatch
	VESPER_SHARES="$(CURDIR)/run/WOR-39/share $(CURDIR)/run/WOR-39/share-ro:ro $(CURDIR)/run/WOR-39/share-nowatch:nowatch" bash scripts/systemvm/generate-launch-config.sh
	bash scripts/systemvm/up.sh

binds-suite:
	bash scripts/systemvm/binds-suite.sh

# Persistent System VM data disk (GH #164): restarts the System VM itself; needs `make docker-up`.
data-disk-suite:
	bash scripts/systemvm/data-disk-suite.sh

# GH #194 / ADR 0010: no container is root on the System VM. Restarts the System VM twice at the end.
systemvm-confine-suite:
	bash scripts/systemvm/confine-suite.sh

systemvm-unfs-suite:
	bash scripts/systemvm/unfs-suite.sh

systemvm-microvm-shares-suite:
	bash scripts/systemvm/microvm-shares-suite.sh

systemvm-microvm-binds-suite:
	bash scripts/systemvm/microvm-binds-suite.sh

# TD-048: a shim microVM sized by the task's memory and CPU limits.
systemvm-microvm-size-suite:
	bash scripts/systemvm/microvm-size-suite.sh

# ADR 0011 slice 2: docker run --runtime vesper on the default bridge (needs make systemvm-up && make docker-up).
systemvm-docker-microvm-suite:
	bash scripts/systemvm/docker-microvm-suite.sh

# Path policy of the guest's share notification sink (GH #150); runs without a System VM.
share-notify-selftest:
	bash scripts/systemvm/share-notify-selftest.sh

# The launcher's share watcher in dry-run mode (GH #150); macOS, needs `make vmlauncher-build`, no VM.
share-watch-selftest:
	bash scripts/macos/share-watch-selftest.sh

# The launch config's JSON contract (TD-044); macOS, needs `make vmlauncher-build`, no VM.
launcher-config-contract:
	bash scripts/macos/launcher-config-contract.sh

dns-suite:
	bash scripts/systemvm/dns-suite.sh

workflow-suite:
	bash scripts/systemvm/workflow-suite.sh

crossarch-suite:
	bash scripts/systemvm/crossarch-suite.sh

# ADR 0011 slice 4b: the Phase 4 suites with every container in a microVM (VESPER_DOCKER_RUNTIME=vesper through
# scripts/systemvm/docker-runtime.sh); a failing step must be a recorded deviation. Needs systemvm-up-shares, docker-up.
phase4-vesper:
	bash scripts/systemvm/phase4-vesper.sh

# ADR 0011 slice 4c: run and stop latency (runc and vesper), density, containerd restart and an exit while it is down.
docker-microvm-measure:
	bash scripts/systemvm/docker-microvm-measure.sh

# D-099: the owner's gate for vesper as dockerd's default, measured with vesper as the real default (restarts the
# System VM twice and restores its launch config).
vesper-default-gate:
	bash scripts/systemvm/vesper-default-gate.sh

vesper-overcommit-check:
	bash scripts/systemvm/vesper-overcommit-check.sh

ctr-conformance:
	bash scripts/systemvm/ctr-conformance.sh $(CTR_FLAGS)

ctr-conformance-loop:
	bash scripts/systemvm/ctr-conformance-loop.sh $(LOOP_N) $(LOOP_LIMIT)

ctr-conformance-check:
	python3 -I scripts/systemvm/ctr_conformance_report.py --self-test
	python3 -I scripts/systemvm/ctr_conformance_report.py --validate docs/specs/ctr-conformance-baseline.json

systemvm-reliability:
	bash scripts/systemvm/reliability-suite.sh $(CYCLES)

# TD-005: the reliability suite and the System VM suites on this Mac, as one evidence bundle (PUBLISH=1 copies it to
# docs/generated/nightly/ to commit; docs/process/nightly-evidence.md).
nightly:
	bash scripts/systemvm/nightly.sh

third-party-notices:
	python3 scripts/licenses/third_party_notices.py

third-party-notices-check:
	python3 scripts/licenses/third_party_notices.py --check

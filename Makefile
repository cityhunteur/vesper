CARGO ?= cargo
RUN_DIR ?= run
CYCLES ?= 50

.PHONY: build test contract-generate contract-check contract-test surface-matrix-generate \
	surface-matrix-check behavior-matrix-generate behavior-matrix-check \
	validate-entitlements dev-up dev-down vmlauncher-build systemvm-fetch systemvm-overlay systemvm-prepare systemvm-build systemvm-launch-config \
	systemvm-boot-smoke systemvm-up systemvm-down systemvm-control-suite systemvm-services-suite systemvm-trivial-image docker-up docker-down docker-smoke docker-conformance ports-suite binds-suite share-notify-selftest share-watch-selftest systemvm-up-shares dns-suite workflow-suite crossarch-suite ctr-conformance ctr-conformance-check systemvm-reliability systemvm-microvm-suite systemvm-rootfs-suite systemvm-workload-suite systemvm-parity-baseline parity-baseline-check shim-build shim-test systemvm-exec-suite systemvm-shim-suite systemvm-bundle-suite systemvm-restart-suite

build:
	$(CARGO) build --bins

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

systemvm-rootfs-suite:
	$(CARGO) run --bin systemvmctl -- control rootfs-suite

systemvm-workload-suite:
	$(CARGO) run --bin systemvmctl -- control workload-suite

systemvm-parity-baseline:
	bash scripts/systemvm/parity-baseline.sh $(PERF_CYCLES) $(PARITY_FLAGS)

shim-build:
	bash scripts/systemvm/build-shim.sh

shim-test:
	cd shim && go vet ./... && go test ./...

systemvm-exec-suite:
	$(CARGO) run --bin systemvmctl -- control exec-suite

systemvm-shim-suite:
	$(CARGO) run --bin systemvmctl -- control shim-suite

systemvm-bundle-suite:
	$(CARGO) run --bin systemvmctl -- control bundle-suite

systemvm-restart-suite:
	$(CARGO) run --bin systemvmctl -- control restart-suite

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

# Path policy of the guest's share notification sink (GH #150); runs without a System VM.
share-notify-selftest:
	bash scripts/systemvm/share-notify-selftest.sh

# The launcher's share watcher in dry-run mode (GH #150); macOS, needs `make vmlauncher-build`, no VM.
share-watch-selftest:
	bash scripts/macos/share-watch-selftest.sh

dns-suite:
	bash scripts/systemvm/dns-suite.sh

workflow-suite:
	bash scripts/systemvm/workflow-suite.sh

crossarch-suite:
	bash scripts/systemvm/crossarch-suite.sh

ctr-conformance:
	bash scripts/systemvm/ctr-conformance.sh $(CTR_FLAGS)

ctr-conformance-check:
	python3 -I scripts/systemvm/ctr_conformance_report.py --self-test
	python3 -I scripts/systemvm/ctr_conformance_report.py --validate docs/specs/ctr-conformance-baseline.json

systemvm-reliability:
	bash scripts/systemvm/reliability-suite.sh $(CYCLES)

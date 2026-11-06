CARGO ?= cargo
RUN_DIR ?= run
CYCLES ?= 50

.PHONY: build test contract-generate contract-check contract-test surface-matrix-generate \
	surface-matrix-check behavior-matrix-generate behavior-matrix-check \
	validate-entitlements dev-up dev-down vmlauncher-build systemvm-fetch systemvm-overlay systemvm-prepare systemvm-build systemvm-launch-config \
	systemvm-boot-smoke systemvm-up systemvm-down systemvm-control-suite systemvm-services-suite systemvm-trivial-image docker-up docker-down docker-smoke systemvm-reliability

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

systemvm-trivial-image:
	bash scripts/systemvm/build-trivial-image.sh

docker-up:
	bash scripts/systemvm/docker-up.sh

docker-down:
	bash scripts/systemvm/docker-down.sh

docker-smoke:
	bash scripts/systemvm/docker-smoke.sh

systemvm-reliability:
	bash scripts/systemvm/reliability-suite.sh $(CYCLES)

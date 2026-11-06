CARGO ?= cargo
RUN_DIR ?= run

.PHONY: build test contract-generate contract-check contract-test surface-matrix-generate \
	surface-matrix-check behavior-matrix-generate behavior-matrix-check \
	validate-entitlements dev-up dev-down vmlauncher-build systemvm-fetch systemvm-build systemvm-launch-config \
	systemvm-boot-smoke

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

systemvm-build:
	bash scripts/systemvm/build-image.sh

systemvm-launch-config:
	bash scripts/systemvm/generate-launch-config.sh

systemvm-boot-smoke:
	bash scripts/systemvm/boot-smoke.sh

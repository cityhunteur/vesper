CARGO ?= cargo
RUN_DIR ?= run

.PHONY: build test contract-generate contract-check contract-test validate-entitlements \
	dev-up dev-down systemvm-build systemvm-launch-config systemvm-boot-smoke

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

validate-entitlements:
	bash scripts/ci/validate-entitlements.sh

dev-up:
	bash scripts/macos/dev-up.sh

dev-down:
	bash scripts/macos/dev-down.sh

systemvm-build:
	bash scripts/systemvm/build-image.sh

systemvm-launch-config:
	bash scripts/systemvm/generate-launch-config.sh

systemvm-boot-smoke:
	bash scripts/systemvm/boot-smoke.sh

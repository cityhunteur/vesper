# Contributing to Vesper

Thank you for helping. Vesper is a parity project: a change is done when it is proved, not when it compiles. This
guide is short; the detail lives in the documents it links.

## Before you start

- Read [AGENTS.md](AGENTS.md). It is written for coding agents and humans alike: the invariants, where to look and
  what needs the maintainer's agreement first.
- For anything larger than a small fix, open or find an issue first and say what you plan. Releases, packaging,
  new host-visible sockets or entitlements, new runtime dependencies and licence changes always need the
  maintainer's agreement before work starts.
- Set up your machine with [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Runtime suites need an Apple silicon Mac,
  M3 or later.

## How a change is made

1. **One small, verifiable unit per pull request**, linked to its issue (`Fixes #123` or `Part of #123`).
2. **Code, tests and docs together.** A behaviour change comes with a regression test. Documentation that
   contradicts the code is a bug; fix one or the other in the same change.
3. **Check narrowly, then broadly.** Run the tests that cover what you touched, then the broader checks:

   ```sh
   cargo test
   make contract-check contract-test surface-matrix-check behavior-matrix-check
   cargo run --bin compatexctl -- --registry docs/specs/compat-exceptions.yaml
   python3 harness/check_docs.py
   python3 scripts/harness/check_structure.py --root .
   make third-party-notices-check     # if Cargo.lock or shim/go.sum changed
   ```

   Runtime-sensitive changes (the System VM, the shim, the proxy) also need the matching `make systemvm-*`,
   `make docker-conformance` or `make ctr-conformance` suite, with its output in the pull request.
   [docs/TESTING.md](docs/TESTING.md) says which suite covers what.
4. **Parity claims cite the pinned revision** in
   [docs/specs/design-v0.baselines.lock.yaml](docs/specs/design-v0.baselines.lock.yaml). A difference from
   Firecracker is never silent: record it as a compatibility exception
   ([process](docs/process/compat-exceptions.md)).
5. **Keep knowledge in the repository.** Decisions go in the [decision log](docs/design-docs/decision-log.md) or an
   ADR, multi-step work in an [ExecPlan](docs/PLANS.md), and new debt in the
   [tech-debt tracker](docs/exec-plans/tech-debt-tracker.md).
6. **CI must be green.** Local green is not remote green.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) with the issue number, for
example `fix(proxy): keep half-closed relays open (#167)`.

## Sign-off and licensing of contributions

Every commit must carry a `Signed-off-by:` line (`git commit -s`), which certifies the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the change or otherwise
have the right to submit it.

Vesper is licensed under the [Business Source License 1.1](LICENSE), and each version converts to the Apache
License 2.0 on its Change Date. By submitting a contribution you agree that:

- it is licensed to everyone under the same terms as Vesper, including the conversion to Apache-2.0; and
- you also grant the Licensor (Pravin Goomannee) a perpetual, worldwide, non-exclusive, royalty-free, irrevocable
  licence to use, reproduce, modify, sublicense and distribute it under any terms, including commercial licences of
  Vesper. You keep the copyright in your contribution.

If you cannot agree to this, please open an issue to discuss it before you submit code.

## Reporting bugs and security issues

Use the issue templates for bugs and feature requests. Do not report vulnerabilities in public issues; follow the
[security policy](.github/SECURITY.md).

## Conduct

Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).

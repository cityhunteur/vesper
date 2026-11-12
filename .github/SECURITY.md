# Security policy

## Supported versions

Vesper is pre-alpha and has no releases yet. Security fixes are made on `main` only.

## Reporting a vulnerability

Please report vulnerabilities privately, not in public issues:

1. Open [a private vulnerability report](https://github.com/cityhunteur/vesper/security/advisories/new)
   (repository **Security** tab → **Report a vulnerability**).
2. Include the affected component (`vesperd`, `vesper-proxy`, `vesper-vmlauncher`, the shim, the System VM image),
   the commit, steps to reproduce and the impact you expect.

You will get an acknowledgement within 7 days and a plan or a decision within 30 days. Please give us a reasonable
time to fix the issue before you disclose it. Credit is given in the advisory unless you ask otherwise.

## Scope and threat model

The threat model, the trust boundaries (host, System VM, microVM, container) and the controls that are and are not
in place are in [docs/SECURITY.md](../docs/SECURITY.md). Gaps already recorded there or in the
[tech-debt tracker](../docs/exec-plans/tech-debt-tracker.md) are known, but reports that show a practical exploit
of them are still welcome.

# Vesper Core Beliefs

These beliefs translate agent-first harness engineering into Vesper’s parity-sensitive runtime environment.

1. **Humans steer. Agents execute.**
   Humans decide goals, priorities, and acceptance criteria. The harness exists so agents can execute safely and repeatably.

2. **The repository is the system of record.**
   If an agent cannot find it in the repo, it effectively does not exist.

3. **`AGENTS.md` is a table of contents.**
   Keep it short. Put deep context in linked docs.

4. **A small proven slice beats a large optimistic claim.**
   Each issue should map to one smallest verifiable parity or harness unit.

5. **A reachable route is not parity.**
   Route presence without correct state behavior and error semantics is incomplete.

6. **Runtime claims need runtime evidence.**
   Proxy and System VM work must leave logs, reports, or other durable artifacts.

7. **Prefer boring, inspectable dependencies.**
   If a tiny repo-local abstraction is clearer than an opaque helper package, prefer the repo-local version.

8. **When the agent struggles, upgrade the harness.**
   Missing docs, scripts, structure checks, or artifact capture should be fixed directly.

9. **Human taste should compound.**
   Repeated review comments belong in docs, templates, scripts, or workflows.

10. **Continuous garbage collection beats debt clean-up marathons.**
    Scan for drift on a cadence, track quality, and land small fix-ups continuously.

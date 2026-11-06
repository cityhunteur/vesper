# Doc-gardening prompt: keep documents true to the code

Run this on a schedule (weekly is a good start), or after a large merge. It finds documents that no longer describe
the code, and fixes them in small PRs. It is the documentation half of continuous garbage collection. The code half is
[entropy-sweep.md](entropy-sweep.md).

---

You are maintaining this repository's knowledge base. Start at `AGENTS.md` and `docs/README.md`. Your goal is to find
documentation that no longer matches the code, and to correct it, without changing application code.

1. Run `python3 harness/check_docs.py` and fix any errors first: broken links, orphaned documents, missing index entries.
2. List the documents whose front matter `last_reviewed` is oldest, plus any document whose linked code paths
   changed since that date. Check with `git log --since=<last_reviewed> -- <paths>`.
3. For each candidate, check every **Verified** claim against the current code: run the documented commands,
   confirm the paths and names exist, and compare the described flows with the implementation.
4. Correct what is wrong, and bump `last_reviewed`. If you cannot verify a claim, relabel it **Unknown** and say why.
   If the code is wrong and the document is right, don't change the code. Add a tech-debt entry instead.
5. Look for knowledge that agents needed but could not find. Check recent PR discussions, review comments and
   ExecPlan Surprises & Discoveries entries. Write it into the document that owns the subject.
6. Open one small PR per subject area. Each PR description lists the claims checked, what changed and the evidence.

Keep changes minimal and reviewable in under a minute. Don't rewrite documents for style alone.

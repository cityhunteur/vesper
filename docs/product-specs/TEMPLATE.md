---
status: draft
owner: <team or person>
last_reviewed: YYYY-MM-DD
---

# <Capability or user journey>

## Purpose and actors

Why this capability exists, and who uses it: which users, systems or scheduled jobs.

## Behaviours

What the system does, one behaviour per paragraph or bullet. Label each **Verified**, **Intended** or **Unknown**.

## Preconditions

What must be true before each behaviour can happen: authentication, state, configuration.

## Inputs and outputs

Requests, events, files or UI actions coming in, and the responses, side effects and events going out.
Link to the schema or contract instead of copying it.

## Business rules and invariants

Rules that must always hold, each with a link to the code or test that enforces it.

## Error scenarios

What can go wrong, what the user or caller sees, and what state remains afterwards.

## Acceptance criteria

Observable checks that prove the capability works: "Given…, when…, then…".

## APIs, code and tests

Links to the endpoints, handlers, modules and tests that implement and verify this spec.

---
name: linear
description: |
  Use Symphony's `linear_graphql` client tool for raw Linear GraphQL
  operations such as comment editing and upload flows.
---

# Linear GraphQL

Use this skill for raw Linear GraphQL work during Symphony app-server sessions.

If the current workflow enables `tracker.auto_manage_queue`, assume Symphony's
orchestrator already owns routine `Backlog -> Todo -> In Progress` movement.
Use this skill to verify tracker truth, repair mismatches, post evidence-rich
updates, and handle milestone states such as `In Review`, `Human Review`, or
`Done` rather than duplicating every routine queue transition manually.

## Primary tool

Use the `linear_graphql` client tool exposed by Symphony's app-server session.
It reuses Symphony's configured Linear auth for the session.

Tool input:

```json
{
  "query": "query or mutation document",
  "variables": {
    "optional": "graphql variables object"
  }
}
```

Tool behavior:

- Send one GraphQL operation per tool call.
- Treat a top-level `errors` array as a failed GraphQL operation even if the
  tool call itself completed.
- Keep queries/mutations narrowly scoped; ask only for the fields you need.

## Discovering unfamiliar operations

When you need an unfamiliar mutation, input type, or object field, use targeted
introspection through `linear_graphql`.

List mutation names:

```graphql
query ListMutations {
  __type(name: "Mutation") {
    fields {
      name
    }
  }
}
```

Inspect a specific input object:

```graphql
query CommentCreateInputShape {
  __type(name: "CommentCreateInput") {
    inputFields {
      name
      type {
        kind
        name
        ofType {
          kind
          name
        }
      }
    }
  }
}
```

## Common workflows

### Query an issue by key, identifier, or id

Use these progressively:

- Start with `issue(id: $key)` when you have a ticket key such as `MT-686`.
- Fall back to `issues(filter: ...)` when you need identifier search semantics.
- Once you have the internal issue id, prefer `issue(id: $id)` for narrower reads.

Lookup by issue key:

```graphql
query IssueByKey($key: String!) {
  issue(id: $key) {
    id
    identifier
    title
    state {
      id
      name
      type
    }
    project {
      id
      name
    }
    branchName
    url
    description
    updatedAt
    links {
      nodes {
        id
        url
        title
      }
    }
  }
}
```

Lookup by identifier filter:

```graphql
query IssueByIdentifier($identifier: String!) {
  issues(filter: { identifier: { eq: $identifier } }, first: 1) {
    nodes {
      id
      identifier
      title
      state {
        id
        name
        type
      }
      project {
        id
        name
      }
      branchName
      url
      description
      updatedAt
    }
  }
}
```

Resolve a key to an internal id:

```graphql
query IssueByIdOrKey($id: String!) {
  issue(id: $id) {
    id
    identifier
    title
  }
}
```

Read the issue once the internal id is known:

```graphql
query IssueDetails($id: String!) {
  issue(id: $id) {
    id
    identifier
    title
    url
    description
    state {
      id
      name
      type
    }
    project {
      id
      name
    }
    attachments {
      nodes {
        id
        title
        url
        sourceType
      }
    }
  }
}
```

### Query team workflow states for an issue

Use this before changing issue state when you need the exact `stateId`:

```graphql
query IssueTeamStates($id: String!) {
  issue(id: $id) {
    id
    team {
      id
      key
      name
      states {
        nodes {
          id
          name
          type
        }
      }
    }
  }
}
```

### Edit an existing comment

Use `commentUpdate` through `linear_graphql`:

```graphql
mutation UpdateComment($id: String!, $body: String!) {
  commentUpdate(id: $id, input: { body: $body }) {
    success
    comment {
      id
      body
    }
  }
}
```

### Create a comment

Use `commentCreate` through `linear_graphql`:

```graphql
mutation CreateComment($issueId: String!, $body: String!) {
  commentCreate(input: { issueId: $issueId, body: $body }) {
    success
    comment {
      id
      url
    }
  }
}
```

### Move an issue to a different state

Use `issueUpdate` with the destination `stateId`:

```graphql
mutation MoveIssueToState($id: String!, $stateId: String!) {
  issueUpdate(id: $id, input: { stateId: $stateId }) {
    success
    issue {
      id
      identifier
      state {
        id
        name
      }
    }
  }
}
```

### Query same-project issues for queue maintenance

Use the same project slug that Symphony is already polling so queue decisions are
based on the same candidate set as the orchestrator:

```graphql
query ProjectQueueIssues(
  $projectSlug: String!
  $stateNames: [String!]!
  $first: Int!
  $relationFirst: Int!
) {
  issues(
    filter: {
      project: { slugId: { eq: $projectSlug } }
      state: { name: { in: $stateNames } }
    }
    first: $first
  ) {
    nodes {
      id
      identifier
      title
      priority
      state {
        id
        name
        type
      }
      project {
        id
        name
      }
      inverseRelations(first: $relationFirst) {
        nodes {
          type
          issue {
            id
            identifier
            state {
              id
              name
              type
            }
          }
        }
      }
      createdAt
      updatedAt
    }
  }
}
```

Normalize blockers the same way Symphony does:

- Treat inverse relation type `blocks` as "this issue is blocked by that related
  issue".
- A `Backlog` issue is ready only when every blocker is terminal (`Done`,
  `Closed`, `Cancelled`, `Canceled`, or `Duplicate`) or there are no blockers.

### Attach a GitHub PR to an issue

Use the GitHub-specific attachment mutation when linking a PR:

```graphql
mutation AttachGitHubPR($issueId: String!, $url: String!, $title: String) {
  attachmentLinkGitHubPR(
    issueId: $issueId
    url: $url
    title: $title
    linkKind: links
  ) {
    success
    attachment {
      id
      title
      url
    }
  }
}
```

If you only need a plain URL attachment and do not care about GitHub-specific
link metadata, use:

```graphql
mutation AttachURL($issueId: String!, $url: String!, $title: String) {
  attachmentLinkURL(issueId: $issueId, url: $url, title: $title) {
    success
    attachment {
      id
      title
      url
    }
  }
}
```

### Introspection patterns used during schema discovery

Use these when the exact field or mutation shape is unclear:

```graphql
query QueryFields {
  __type(name: "Query") {
    fields {
      name
    }
  }
}
```

```graphql
query IssueFieldArgs {
  __type(name: "Query") {
    fields {
      name
      args {
        name
        type {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
            }
          }
        }
      }
    }
  }
}
```

### Upload a video to a comment

Do this in three steps:

1. Call `linear_graphql` with `fileUpload` to get `uploadUrl`, `assetUrl`, and
   any required upload headers.
2. Upload the local file bytes to `uploadUrl` with `curl -X PUT` and the exact
   headers returned by `fileUpload`.
3. Call `linear_graphql` again with `commentCreate` (or `commentUpdate`) and
   include the resulting `assetUrl` in the comment body.

Useful mutations:

```graphql
mutation FileUpload(
  $filename: String!
  $contentType: String!
  $size: Int!
  $makePublic: Boolean
) {
  fileUpload(
    filename: $filename
    contentType: $contentType
    size: $size
    makePublic: $makePublic
  ) {
    success
    uploadFile {
      uploadUrl
      assetUrl
      headers {
        key
        value
      }
    }
  }
}
```

## Usage rules

- Use `linear_graphql` for comment edits, uploads, and ad-hoc Linear API
  queries.
- Prefer the narrowest issue lookup that matches what you already know:
  key -> identifier search -> internal id.
- For state transitions, fetch team states first and use the exact `stateId`
  instead of hardcoding names inside mutations.
- Prefer `attachmentLinkGitHubPR` over a generic URL attachment when linking a
  GitHub PR to a Linear issue.
- Do not introduce new raw-token shell helpers for GraphQL access.
- If you need shell work for uploads, only use it for signed upload URLs
  returned by `fileUpload`; those URLs already carry the needed authorization.

## Progress bookkeeping protocol

Keep Linear truthful. Do not only update the current issue; maintain the next
ready queue as well.

### Current issue state

- Move `Todo -> In Progress` only after baseline capture or initial repro is
  recorded.
- Do not move an issue into `In Review`, `Human Review`, `Done`, or another
  completed downstream state while the accepted work exists only as a local
  dirty diff. Commit accepted work first, and prefer a pushed branch or PR for
  review states.
- Move to `In Review`, `Human Review`, `Done`, or another downstream state only
  when the evidence for that state already exists in the repo, PR, and tracker
  comment.
- If the code and Linear disagree, fix Linear to match the code and explain the
  correction in the progress comment or workpad.

### Queue maintenance

When `tracker.auto_manage_queue` is enabled, treat the orchestrator as the
primary queue manager. The planner should not churn states redundantly, but it
must repair the queue when:

- automation is disabled,
- automation failed to move the next ready ticket,
- or the project is idle while ready backlog work exists.

Before ending a run, parking a ticket in a non-active review state, or closing a
ticket, reconcile the next ready work in the same project:

1. Query same-project issues in `Backlog`, `Todo`, `In Progress`, and any local
   review/checkpoint states used by the workflow.
2. Identify ready `Backlog` issues:
   - state is exactly `Backlog`
   - all explicit blockers are terminal
   - no higher-priority ready issue in the same dependency frontier should go
     first
3. Promote the next ready issue(s) from `Backlog -> Todo` with `issueUpdate`.
4. Default to the smallest truthful queue:
   - for serial dependency chains, promote only the single next issue
   - only promote multiple issues when the workflow intentionally supports
     parallel execution and the issues are independently ready
5. If a `Todo` issue is discovered to be blocked again before work actually
   starts, move it back to `Backlog`.
6. Mention every queue change in the tracker update so humans can see why an
   issue was promoted or parked.

### Ordering rules

When choosing which ready `Backlog` issue to promote:

1. Dependency order first: an issue unblocked by the current work should go
   ahead of unrelated future work.
2. Then priority: lower numeric Linear priority wins (`1` before `2`, etc.).
3. Then age: older created issues win when priority is tied.

Do not leave the next dependency slice stranded in `Backlog` when it is now the
truthful next item.

# AGENTS.md

This file defines repository-wide instructions for AI coding agents.

These instructions apply to all implementation, debugging, refactoring,
testing, and verification work in this repository.

---

## Core Rule

Continue working until the requested task is actually completed and verified,
or until a genuine blocker requires user input.

Do not stop merely because you have identified the next step.

If the next action can be performed using the available tools, perform it.

---

## Execution Loop

For implementation tasks, continuously follow:

```text
Inspect
  ↓
Understand
  ↓
Implement
  ↓
Verify
  ↓
Failure?
 ├─ Yes → Diagnose → Fix → Verify again
 └─ No  → Review → Complete
````

Continue this loop automatically.

Do not require the user to say:

* continue
* go
* proceed
* next
* yes

between ordinary development steps.

---

## Do Not Stop at Planning

Do not end a response with statements such as:

* "Let me inspect the implementation."
* "I'll examine the relevant files."
* "I need to investigate this bug."
* "Next I need to run the tests."
* "Let me understand the current state."
* "I'll proceed with verification."
* "The next step is..."
* "I will now..."

If you can perform that action, perform it immediately.

Planning is not completion.

---

## Repository Inspection

Before modifying code:

1. Inspect the relevant files.
2. Search for related symbols and call sites.
3. Inspect relevant tests.
4. Understand the existing architecture.
5. Identify the smallest appropriate change.

When a specific file or line is mentioned, inspect it immediately.

Do not guess repository behavior that can be determined from the source.

---

## Debugging

When encountering a bug:

1. Reproduce or inspect the failure.
2. Read the relevant implementation.
3. Identify the root cause.
4. Apply the smallest appropriate fix.
5. Run the relevant verification.
6. Inspect any failures.
7. Fix them.
8. Repeat until verification passes.

A compiler error, failing test, lint error, or runtime error is not normally
a reason to stop.

It is information for the next debugging iteration.

---

## Existing Blocking Bugs

If an existing bug blocks the requested task:

```text
Original task
    ↓
Blocking bug
    ↓
Investigate
    ↓
Fix
    ↓
Verify fix
    ↓
Resume original task
```

Do not forget the original objective after resolving a subproblem.

---

## Implementation

Prefer:

* minimal diffs
* existing architecture
* existing abstractions
* existing naming conventions
* existing error-handling patterns
* existing dependencies

Avoid unrelated refactoring.

Do not rewrite working components merely because another implementation is
possible.

---

## Verification

After making changes, verify them immediately.

Use the project's actual tooling.

Examples may include:

```bash
cargo check
cargo test
cargo clippy

npm test
npm run lint
npm run build

pnpm test

pytest

go test ./...
```

Do not blindly execute every command above. Determine the appropriate commands
from the repository.

Prefer:

```text
targeted verification
        ↓
module/package verification
        ↓
broader project verification
```

when appropriate.

---

## Automatic Recovery

If verification fails:

```text
Read error
   ↓
Locate cause
   ↓
Fix
   ↓
Run verification again
   ↓
Repeat
```

Do not ask the user for permission to fix ordinary implementation errors.

Do not stop merely to report an error that can be investigated locally.

---

## Phase and Milestone Work

When the repository uses phases or milestones, do not consider a phase complete
merely because the expected files exist.

Verify:

* required functionality
* actual implementation
* integration
* tests
* error handling
* acceptance criteria

If verification discovers a bug, investigate and fix it before continuing.

For example:

```text
Phase 3 verification
        ↓
metadata.rs:70 bug
        ↓
inspect
        ↓
find root cause
        ↓
fix
        ↓
test
        ↓
resume Phase 3 verification
        ↓
complete Phase 3
```

---

## Maintain the Parent Objective

For long-running tasks, continuously retain:

* original objective
* current step
* completed steps
* remaining steps
* known failures
* verification status

A debugging branch must not replace the original task.

After resolving the branch, resume the parent objective.

---

## Tool Usage

Use available repository, filesystem, search, terminal, and test tools
proactively.

Prefer executing an available action over explaining that the action should be
performed.

Bad:

> I need to inspect metadata.rs.

Correct behavior:

> Inspect metadata.rs using the available tools and continue.

Bad:

> We should run cargo test next.

Correct behavior:

> Run the appropriate cargo test command and inspect the result.

---

## Permissions

Do not request confirmation for normal, reversible development operations such
as:

* reading files
* searching source code
* inspecting Git state
* editing project source files
* running builds
* running tests
* running linters
* running static analysis
* fixing compiler errors
* fixing test failures caused by the implementation

Proceed automatically when these actions are necessary to complete the task.

---

## Safety

Do not automatically perform destructive, irreversible, production, or
externally visible operations.

Ask before actions such as:

* force pushing
* rewriting shared Git history
* deleting substantial user data
* destructive database migrations
* production deployment
* publishing packages or releases
* changing production infrastructure
* deleting remote resources
* rotating credentials or secrets

Prefer reversible local operations.

---

## Git

When Git is available, inspect the final diff before declaring completion.

Check for:

* accidental modifications
* debug output
* temporary files
* unrelated formatting changes
* secrets
* commented-out experimental code
* unintended generated files

Do not commit or push unless explicitly requested.

---

## Valid Stop Conditions

Stop only when one of the following is true:

### 1. Completed

The requested objective is implemented and sufficiently verified.

### 2. External Blocker

Required information or resources are unavailable, such as:

* credentials
* inaccessible external service
* unavailable repository/resource
* required hardware
* information only the user can provide

State exactly what is missing.

### 3. Material User Decision

A high-impact architectural or product decision cannot safely be inferred from
the repository or task.

Explain the concrete decision required.

### 4. Unsafe Operation

The next required operation is destructive or irreversible and requires user
approval.

---

## Invalid Stop Conditions

These are NOT valid reasons to stop:

* "I found the bug."
* "I know the next step."
* "I need to inspect another file."
* "I need to run the tests."
* "The tests failed."
* "There is a compiler error."
* "There is another bug."
* "I need to verify the implementation."
* "Let me investigate."
* "Let me start by understanding the current state."
* "I'll proceed with the verification."

If a safe next action is available, execute it.

---

## Completion Standard

Before declaring completion, confirm:

1. The requested behavior is implemented.
2. Relevant source code has been inspected.
3. The change has been verified.
4. Relevant tests/checks pass, or unrelated pre-existing failures are clearly
   identified.
5. No known required implementation step remains.
6. The final diff contains no obvious accidental changes.

Only then provide the final report.

---

## Final Report

When work is complete, report concisely:

* what changed
* root cause, when debugging
* verification performed
* test/build result
* any genuine remaining issue

Do not end with a description of a next step that you could perform yourself.

---

## Prime Directive

**Inspect → Implement → Verify → Diagnose → Fix → Re-verify → Complete**

Continue execution while safe, relevant work remains.

Do not confuse planning with execution.

Do not confuse explanation with completion.

Do not require the user to repeatedly tell you to continue.

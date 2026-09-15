# Safety Envelope and Decision Authority

The safety-authority layer decides whether a proof-linked correction proposal may actually be applied.

It intentionally separates four questions:

1. **What happened?** — answered by evidence and `BeadCommit`.
2. **How far did reality diverge from intention?** — answered by `OrientationDelta`.
3. **What correction is proposed?** — answered by `CorrectionPolicy`.
4. **Who has authority to execute it?** — answered by `DecisionAuthority`.

The fourth question must not rewrite the first three.

## Control picture

```text
proof-backed movement
        |
        v
OrientationDelta
        |
        v
CorrectionPolicy
        |
        v
CorrectionDecision
        |
        v
DecisionAuthority
   /      |       \
allow   approval   block
```

## Core invariants

```text
approval UNKNOWN != approval DENIED
human approval != missing evidence
soft envelope != hard envelope
permission to execute != proof that execution is correct
```

A human approval may authorize a policy-bounded action, but it cannot manufacture missing evidence. Likewise, a quorum can grant execution authority without changing the epistemic state of the underlying claim.

## Autonomy levels

`AutonomyLevel` has three initial modes:

- `ObserveOnly` — inspect and explain, but never execute;
- `RecommendOnly` — produce a recommendation, but require approval before execution;
- `BoundedAutomatic` — allow small proof-backed corrections to execute automatically inside the configured safety envelope.

The autonomy level answers an authority question, not an evidence question.

## Approval state

Human approval is explicitly three-valued:

- `Unknown` — approval has not been resolved;
- `Approved` — a human authorized execution;
- `Denied` — a human rejected execution.

This preserves the same discipline used elsewhere in Lifetra:

```text
UNKNOWN != FALSE
```

Pending approval must not be interpreted as rejection, and rejection must not be silently reinterpreted as pending.

## Safety envelope

`SafetyEnvelope` defines:

- `min_proof_refs` — minimum proof references required before authority is even considered;
- `quorum_required` — independent approvals required by the surrounding policy;
- `require_human_approval` — explicit human gate;
- `max_autonomous_adjustment` — soft limit for automatic execution;
- `hard_max_adjustment` — non-overridable execution limit;
- `autonomy` — the execution authority mode.

The ordering invariant is:

```text
max_autonomous_adjustment <= hard_max_adjustment
```

## Soft versus hard boundaries

A correction above `max_autonomous_adjustment` but below the hard limit may be escalated to human approval.

A correction above `hard_max_adjustment` is blocked even if a human has approved it through this API. The hard boundary represents a safety invariant that must be changed explicitly in policy, rather than bypassed at runtime.

Conceptually:

```text
small correction      -> automatic
larger safe correction -> approval path
outside hard envelope  -> block
```

## Proof gate

Insufficient proof is blocking:

```text
proof_count < min_proof_refs -> BLOCK
```

This is deliberate. A human or quorum may authorize action, but they cannot convert an unproven observation into proof.

## Quorum

`quorum_approvals` is independent from human approval.

This allows deployments where authority may require, for example:

- two independent policy agents;
- one human plus one automated reviewer;
- multiple organizational sign-offs;
- zero quorum for low-risk bounded automation.

The MVP counts approvals but does not yet model signer identity, independence, weight, expiry, or cryptographic receipts. Those are future extensions.

## Verdicts

`DecisionAuthority::evaluate()` produces one of:

- `Allow(Automatic)` — the correction is proof-backed and inside automatic authority;
- `Allow(ApprovedManual)` — required approval has been satisfied;
- `RequireApproval` — the correction is not rejected, but authority is incomplete;
- `Block` — execution is prohibited by epistemic or hard safety constraints.

The result also carries reasons such as:

- insufficient proof;
- quorum pending;
- human approval required;
- human approval denied;
- outside autonomous envelope;
- outside hard envelope;
- observe-only or recommend-only policy.

## Why `CorrectionDecision` remains a proposal

The correction layer computes a possible next orientation. It does not itself mutate the trajectory or claim execution occurred.

The authority layer therefore evaluates a proposal without turning it into history.

A later execution system should create a new bead and new evidence after the action actually occurs.

That gives the full loop:

```text
intent
  -> action
  -> evidence
  -> proven movement
  -> delta
  -> correction proposal
  -> authority gate
  -> execution
  -> new bead
  -> new evidence
```

The important boundary is that **permission to act is not evidence that the act happened**.

## Current scope

This is an MVP authority model. It does not yet include:

- signer identity and independence;
- approval expiry or revocation;
- weighted quorum;
- policy version receipts;
- tool-specific permissions;
- side-effect classes or economic risk budgets;
- rollback authority;
- multi-step transaction envelopes;
- learned or adaptive safety policy.

Those can be layered later without weakening the current proof and authority boundaries.

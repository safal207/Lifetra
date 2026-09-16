# Lifetra Metro Station v0.1

The Metro station is a narrow JSON boundary between **Liminal Rail Metro** and Lifetra's existing proof-backed control model.

It does not duplicate Metro routing and it does not move Rust logic into Go.

```text
Metro Receipt
    |
    v
lifetra.observation.v0.1
    |
    v
TrajectoryBead
    |
    +-- UNKNOWN / REJECTED --> BLOCK
    |
    v
BeadCommit
    |
    v
ProvenOrientation
    |
    v
OrientationDelta
    |
    v
CorrectionPolicy
    |
    v
DecisionAuthority
    |
    +-- BLOCK
    +-- REQUIRE_APPROVAL
    `-- ALLOW --> lifetra.decision.v0.1 --> Metro Packet
```

## Why this boundary exists

Liminal Rail Metro owns the hot execution path: packet identity, routing, dispatch, and receipts.

Lifetra owns the reflective/control path: evidence-gated trajectory commits, proof-backed movement, bounded correction, and execution authority.

The station connects those roles without making either runtime depend on the other's implementation language.

## Input

The example accepts `lifetra.station.request.v0.1` on stdin. The request contains:

- the existing `lifetra.observation.v0.1` envelope produced from a Metro receipt;
- a Lifetra epoch timestamp for the bounded bead;
- intended orientation;
- observed orientation for confirmed outcomes;
- correction policy bounds;
- safety/authority bounds;
- a proposed *new* Metro action to issue only when authority returns `ALLOW`.

The proposed next action must have a different `action_id` from the action that produced the observed receipt. The prior action remains provenance through `caused_by_action_id` and `source_receipt_ref`.

## UNKNOWN remains UNKNOWN

For a Metro observation with `receipt_status = "UNKNOWN"`, the station creates a bead with unresolved external-effect uncertainty. Lifetra's normal `TrajectoryBead::prove_transition` gate must reject that bead, and the station emits `BLOCK`.

It does not reinterpret missing acknowledgement as success, failure, or retry authority.

## Confirmed outcomes

`SUCCEEDED` and `FAILED` are externally confirmed outcomes. They can support a bounded bead commit when proof is present. The caller supplies the observed movement vector; Lifetra then compares it with intended orientation and computes a bounded correction.

`REJECTED` does not advance the control loop in v0.1 and emits `BLOCK`.

## Authority

The station delegates execution authority to Lifetra's existing `DecisionAuthority`:

- `ALLOW` — emits `lifetra.decision.v0.1` with an authority proof reference and the new action proposal;
- `REQUIRE_APPROVAL` — emits no executable next action;
- `BLOCK` — emits no executable next action.

When allowed, the next action inputs receive a `lifetra` object containing the source bead, alignment score, dominant orientation gap, bounded correction, next intended orientation, and execution mode.

## Run

```bash
cargo run --quiet --example metro_station < request.json
```

The program writes only the `lifetra.decision.v0.1` JSON envelope to stdout. Errors go to stderr and return a non-zero exit code.

## Test

```bash
cargo test --test metro_station_contract
```

The contract tests cover:

1. a proof-backed correction inside the autonomous envelope -> `ALLOW`;
2. an unresolved Metro effect -> `BLOCK`;
3. a correction outside the autonomous envelope but inside the hard envelope -> `REQUIRE_APPROVAL`;
4. accidental reuse of the prior action identity -> rejected fail-closed.

## Claim ceiling

This v0.1 station does **not** claim:

- automatic inference of an orientation vector from arbitrary tool output;
- exactly-once side-effect execution;
- safe automatic redispatch after an unknown outcome;
- cryptographic authority receipts;
- distributed consensus between Lifetra and Metro.

It proves only that Metro execution evidence can cross into Lifetra's existing proof/control path and return a bounded authority decision without weakening the core identity and uncertainty invariants.

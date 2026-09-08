# Standalone multiplayer foundation

Status: initial implementation stage; full gameplay parity remains required.

## Contract

- Independent Rust executable and documented protocol, usable without any model
  provider, agent harness or benchmark framework.
- Reuse OpenFoot football mechanics; do not translate the match engine.
- Explicit manager identities, club permissions and private observations.
- Independent cloned clubs with equal starting football state and resources.
- Persistent progress across seasons; fixed manager assignments. Firing ends a
  participant's control permanently and a bot takes over the club.
- Bots reuse useful OpenFoot heuristics but obey the same transaction, observation
  and authorization rules as other participants; no privileged ownership changes.
- Daily windows close on all-ready or deadline. Ready managers can answer existing
  negotiations but cannot reopen ordinary management or extend the window.
- Atomic FIFO commands, non-blocking previews and confirmation. A confirmation is
  a new command; changed material facts require renewed approval. Waiting for a
  manager never locks world state. This is not day-end auction settlement.
- Fully delegated matches initially; halftime intervention is a later possibility.
- Public snapshot/history/subscription APIs with reconnect cursors and server-side
  privacy. Narration is retrospective, presentation-neutral and outside authoritative
  simulation; private information remains private.

## First bounded change

Import the pinned standalone engine unchanged with upstream tests, record source
provenance and verify it builds independently of the desktop application. No bots,
multiplayer lifecycle or gameplay parity is claimed by this first change.

## Following stages

1. Finish operation-level gameplay inventory and choose explicit rules for preview
   dependencies, offer expiry, registration and automatic match delegation.
2. Add typed multiplayer state, scenario cloning, persistence and protocol.
3. Implement shared management commands and native bot policies incrementally;
   retain squad/tactics, training, scouting/youth, transfers/loans/contracts,
   staff/facilities/finances, conversations/board and season progression coverage.
4. Prove scripted management windows, stale confirmations, consent, atomic funds,
   match results, firing/takeover and save/load/rollover.
5. Implement privacy-filtered public API and a separate narration contract.

Keep prototype completion distinct from the feature-parity release. Do not silently
omit management functions because an earlier external tool surface lacked them.
Characterize reused heuristics; exact old bot outcomes are not a requirement.

## Verification

Imported engine tests first. Later: clone equality without shared references,
cross-manager privacy, deadline races, duplicate receipts, transfer conservation,
independent scouting, no automatic override of external plans, once-only daily
processing, persistent season rollover and reconnectable public event history.

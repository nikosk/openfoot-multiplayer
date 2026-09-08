# Watchable head-to-head delivery

Status: in progress. User priority: watch two external managers competing for a
championship, with a third model narrating public events, as soon as possible.

## Boundaries retained

Standalone GPL game code has no model-provider or evaluation-product dependency.
Two independent equal club/player copies; fixed manager ownership. One persistent
session per external manager, daily all-ready/deadline closure, FIFO commands,
private observations, fully delegated matches. Public retrospective narration is
an external read-only client and cannot block the authoritative game clock.

The existing foundation is not full football parity. Permission to launch a
reduced prototype has been asked explicitly; until answered, implement common
infrastructure without launching a reduced paid contest. Contracts, scouting,
finances, injury lifecycle, board/firing and season rollover remain blockers to a
full-parity release. Do not silently fabricate these mechanics or their outcomes.

## Immediate implementation

1. Add a host-controlled JSON-lines executable around Football: explicit scenario
   initialization, scoped observation/commands, host tick and public snapshot.
   It is a local trusted-host transport, not an unauthenticated network API.
   Host assigns manager identity and wall-clock time; model-facing clients may
   never invoke init/tick/save or choose another actor identity.
2. Retain exact accepted host inputs in a private journal with replay tests; no
   model traces in game state. Preserve idempotency and results on reconstruction.
3. Add explicit calendar construction and source-world scenario conversion, not
   a hidden substitute world. Keep original world/shards immutable. Record clone
   mappings and source provenance; document which attributes the prototype uses.
4. External host/client integration must keep model workspaces/credentials/tools
   isolated, expose only own observations plus public history, and stream public
   match results/standings to a presentation-neutral spectator endpoint.
5. Attach the requested exact Luna/DeepSeek low configurations and separate Luna
   low narrator only after a functional launch path and scope are established.

## Checks

Focused correctness checks remain required, but no separate scripted full-season
demonstration is required before the real run. Test malformed input, cross-manager
privacy, atomic day failure, replay/idempotent commands, public-only event payloads,
calendar pairing/venue invariants and clone equality. Run existing workspace tests
and all-target compilation. A run is not launched/completed until actual process
and artifact evidence proves it. Do not replace missing exact models.

## Implemented and verified

The JSON-lines executable, canonical double round robin, explicit source importer,
scoped loopback HTTP host, limited native bot policy, public history/beat endpoints,
independent narration writer, terminal consumer and private journal replay are
implemented. The host enables the eleven-player retained-squad safety rule.
Full gameplay parity, automatic resume and season rollover remain unimplemented.

Verification: 207 Rust tests; 5 importer tests; 4 HTTP/privacy/lifecycle tests;
1 journal replay test; all-target compilation and management formatting pass.
The 20-club normal-world projection initialized successfully with 440 players and
380 scheduled fixtures (last day 266), without playing a match or invoking a model.
The source London club and Liverpool City slot become two London clones; the
other 18 source clubs remain. No original world/shard was modified.

The external model client supplies three isolated persistent sessions and keeps
unchanged long-poll responses inside tools. Paid launch remains pending explicit
prototype-scope confirmation. This is not a completed head-to-head run.

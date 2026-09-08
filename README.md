# OpenFoot Multiplayer

A standalone, headless multiplayer football-management simulator in Rust.

Status: a limited playable prototype with a local host/API, not full OpenFoot
gameplay parity. Includes OpenFoot's match engine and a separate management core. See
[the foundation plan](plans/0001-foundation.md) and
[the transaction slice](plans/0002-management-transactions.md).

The simulator will support human, scripted and external automated managers through
the same rules and protocol. It has no dependency on a particular agent runtime
or evaluation product. Spectating will be API-first, not tied to a graphical client.

Run the imported engine tests with `cargo test --workspace --locked`.

Run the model-free transaction example with
`cargo run -p management --example transfer --locked`.

Run two scripted delegated fixtures and print their results/standings with
`cargo run -p management --example fixtures --locked`. This uses explicit synthetic
squads for a smoke test, not the intended career world. See
[the fixture-slice plan](plans/0003-delegated-fixture-slice.md).

The management core accepts trusted host identity/time and provides private manager
views and public projections. It demonstrates FIFO dispatch, seller consent,
non-blocking previews, atomic confirmation, idempotent receipts and daily closure.
The Rust executable accepts trusted host inputs; `tools/host.mjs` supplies scoped
local HTTP authentication. Client-selected actor IDs are not authentication.
Explicit/generated fixtures resolve at closed daily windows, with a limited bot
policy. Contract lifecycle, private board evaluation/dismissal and continuing
single-league seasons are implemented; full daily football parity remains pending.
Missing/departed selections now receive deterministic lineup repair,
and played minutes carry condition/fitness effects into the next day. Fewer than
eleven available players still prevents resolution. Explicitly configured Rest and
Recovery now restore condition on non-match days using upstream age/morale/staff/
facility factors. Other training focuses, injury persistence and emergency roster
rules must be completed before autonomous careers. The fixtures example supplies
synthetic recovery profiles and verifies an off-day after its two matches.

Managers can also set a private, persistent pre-match plan: six play styles and
the nine engine phase-tactic settings. Both clubs' plans feed delegated matches;
automatic in-match adjustments do not overwrite the saved plan. The example
uses Possession versus Counter. Formation remains 4-4-2; formation-slot mapping,
player-role commands and set pieces are not yet implemented. See
[the tactical slice](plans/0006-persistent-match-tactics.md).

## Local league and spectator prototype

See [the prototype contract and commands](docs/prototype.md). The game host has no
model-provider dependency. Public endpoints drive arbitrary viewers; the included
terminal consumer displays standings and retrospective narration. External clients
must provide model sessions and narrator text. No paid run is part of the tests.

## License and attribution

The repository contains the GNU GPL version 3 in [LICENSE](LICENSE). Imported
OpenFoot code retains its upstream GPLv3-or-later grant and attribution; see
[UPSTREAM.md](UPSTREAM.md). The precise license declaration for newly authored
implementation files will be recorded before adding those files; the license text
alone does not select an “or later” grant.

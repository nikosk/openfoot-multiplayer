# OpenFoot Multiplayer

A standalone, headless multiplayer football-management simulator in Rust.

Status: foundation stage, not yet a playable multiplayer game. Includes OpenFoot's
match engine and a separate management transaction core. See
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
It is not a network server, full transfer/contract system, save format or playable
league yet. The host must authenticate clients; client-selected actor IDs are not
authentication. Explicit fixtures now resolve at closed daily windows. Full daily
football consequences, contracts, calendar generation and native manager bots are
still pending. Missing/departed selections now receive deterministic lineup repair,
and played minutes carry condition/fitness effects into the next day. Fewer than
eleven available players still prevents resolution; injury persistence, training
recovery and emergency roster rules must be completed before autonomous careers.

## License and attribution

The repository contains the GNU GPL version 3 in [LICENSE](LICENSE). Imported
OpenFoot code retains its upstream GPLv3-or-later grant and attribution; see
[UPSTREAM.md](UPSTREAM.md). The precise license declaration for newly authored
implementation files will be recorded before adding those files; the license text
alone does not select an “or later” grant.

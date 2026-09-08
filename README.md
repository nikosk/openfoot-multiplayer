# OpenFoot Multiplayer

### Retrospective broadcast cursors

`GET /beat?after=N` (narrator token) counts **public beat chunks**, not matches.
Each response advances by at most one chunk and provides `next`, `total`,
`cursor_kind: "public_beats"`, completed day/date intervals, public news/events,
and flattened compact results. Quiet preparation flushes after seven closed days;
matches, completion, or failure flush earlier. Each chunk carries at most 20
matches, 20 articles and 20 events, plus standings captured at that interval.
Future-dated imported news is withheld until its date has closed. Public news
may describe rumors; it is not evidence of an executed transfer.

Publish `{after, through: next, text}` to `/narration`, then continue from `next`.
Terminal status does not imply the backlog is drained: continue until
`next === total`. `/history?after=N` remains a separate **raw match** cursor.
Neither missing nor stalled narration blocks a daily transition. Preparation
beats never expose private commands, training choices, conversations or thoughts.

Public historical source-stat rows are separately paginated via
`/player-statistics?player_id=ID&offset=0&limit=20` and
`/team-statistics?club_id=ID&offset=0&limit=20` (maximum 100 rows per request).
These expose committed match statistics, not private player attributes or plans.

`/national?offset=0&limit=20` lists public national-team metadata. Add
`nation_id=ID_OR_CODE` for the selected public roster (identities only), ranking,
and paged fixtures; future fixture scores are withheld. Responses cap fixture/
metadata pages and roster identities at 100. Source world-history records are
separately paged at `/world-history?category=CATEGORY&offset=0&limit=20`, where
category is `rivalries`, `season_awards`, `world_cup_champions`,
`national_team_ranking`, or `world_cup_hosts`. Confirmed future hosts are public
announcements. Neither endpoint returns private call-up conversations or seeds.

Historical awards/statistics retain original source IDs and affiliations.
`/historical-identity?id=ID` resolves removed pre-clone people/clubs to public
identity and career facts. Their full source records persist privately in the
history archive, never as extra signable players, staff, managers or payroll.
Configured league ties preserve the source standings order; the first final
standing is the single champion. Legacy fixture-only worlds retain ID tie order.

A standalone, headless multiplayer football-management simulator in Rust.

Status: integrated gameplay extraction with a local host/API; release verification
is tracked in [plan 0009](plans/0009-gameplay-parity-before-release.md).
Includes OpenFoot's unchanged pinned match engine and a separate management core. See
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
Configured worlds retain training/development, injuries, social conversations,
staff/scouting/youth, finances, contracts/transfers/loans, domestic and national
competition calendars, promotion/qualification, retirement and season history.
Bots use ordinary scoped commands; they have no privileged transaction path.
Missing/departed selections receive source lineup repair, and match wear carries
into following days. Fewer than eleven available players still prevents resolution;
the simulator does not invent emergency signings to conceal a depleted squad.

Managers can also set a private, persistent pre-match plan: six play styles and
the nine engine phase-tactic settings. Both clubs' plans feed delegated matches;
automatic in-match adjustments do not overwrite the saved plan. The example
uses Possession versus Counter. Configured worlds additionally support source
formation slots, player roles, captains and set-piece assignments. See
[the tactical slice](plans/0006-persistent-match-tactics.md).

## Local league and spectator prototype

See [the prototype contract and commands](docs/prototype.md). The game host has no
model-provider dependency. Public endpoints drive arbitrary viewers; the included
terminal consumer displays standings and retrospective narration. External clients
must provide model sessions and narrator text. No paid run is part of the tests.

## License and attribution

The repository contains the GNU GPL version 3 in [LICENSE](LICENSE). Imported
OpenFoot code retains its upstream GPLv3-or-later grant and attribution; see
[UPSTREAM.md](UPSTREAM.md). Newly adapted source modules preserve their
GPLv3-or-later attribution.

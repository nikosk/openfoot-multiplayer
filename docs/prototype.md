# Local multiplayer host and API

The filename is retained from the initial prototype. The configured full-world
path now integrates the gameplay inventory in [plan 0009](../plans/0009-gameplay-parity-before-release.md).
That plan records release verification; the old league-only import remains a
bounded development fixture, not the full-gameplay contest.

## Rules

- The host advances shared days on all-ready or the inclusive deadline.
  Initialization and causal simulation run outside the next decision window.
  Bots and external managers submit through the same FIFO command boundary.
- Ready closes new planning but permits existing transfer negotiations until
  the window closes. Private transfer notices can wake a ready manager that day.
  Previews reserve nothing; confirmation revalidates relevant current facts.
- Matches use the unchanged pinned engine with source formation slots, roles,
  set pieces, saved tactical phases and delegated match decisions. No live
  external match control is exposed. Training, injuries, fitness, morale,
  promises, contracts, scouting and facilities have persistent consequences.
- Paid transfers retain source contract terms. Free-agent contracts have source
  acceptance/counter/rejection rules. Shared seller/buyer consent replaces the
  single-player privileged transaction path. Loans retain wage shares, options,
  registration windows, development and returns.
- Salary field `weekly_wage` preserves the source annual salary unit: Monday
  payroll divides each salary by 52. Gate income, commercial income, sponsorship,
  board support, marketing, debt and ledgers use the selected source rules.
- Native bots are a shared-observation reimplementation with the complete
  management repertoire, not a claim to reproduce every single-player decision.
  Proactive market work respects active clubs; source training covers all clubs.
- Firing permanently revokes the original actor, including receipt replay.
  A caretaker control keeps the club operating; the source person appointment
  occurs after seven daily vacancy sweeps. Managers cannot switch clubs.
- Full worlds retain competition definitions and independent membership.
  Active club matches use the engine; dormant scorelines and national matches
  retain their source lifecycle. Cups, qualification, promotion, aging and
  season history persist. Configured ranking preserves source table order for
  exact ties and awards one championship to the first final-table club.
- No crash/resume promise is made for model sessions. Trusted versioned game
  checkpoints preserve simulator state and receipts. A final save failure is
  an actual host failure, not a reported successful finish.

## Build and import

```sh
cargo build -p management --bin league --release --locked --offline
node tools/import-world.mjs --scope world --world /path/to/manifest.json \
  --competition eng-d1 --source-team SOURCE_ID --replace-team OTHER_SLOT_ID --division-tier 0 \
  --out /path/to/new-private-scenario.json
```

Both slots must belong to the selected league. Their squads, staff and manager
profiles are equal clones of the source club with distinct identities. Other
clubs and free players remain in the shared world. Original referenced shards
are read-only. The importer retains the source calendar and active competition
scope; it does not quietly substitute a smaller world or calibrate club strength.
External actors are `manager:clone-a` and `manager:clone-b`; other actors are bots.

## Run and watch

```sh
node tools/host.mjs --bin /absolute/path/to/target/release/league \
  --scenario /path/to/new-private-scenario.json --out-dir /path/to/new-host-run \
  --port 4319 --day-ms 120000 --max-seasons 1
node tools/watch.mjs http://127.0.0.1:4319
```

Connect clients promptly after the host prints its URL. Its first full decision
window begins after initialization, and bot commands interleave with external
commands. The API drives terminal, web or desktop spectators independently.

The fresh output directory contains private `auth.json`, a fsynced
`journal.jsonl`, public `broadcast.jsonl`, and the terminal private
`final.checkpoint.json`. Never serve credentials, journals or checkpoints to
spectators. The host binds loopback only. SIGINT/SIGTERM close its child and
resources; a completed host remains available for spectators until stopped.
Runtime diagnostics go to private stderr, not public error responses.

## API

| Endpoint | Authority/content |
| --- | --- |
| `GET /observe` | Manager: own club, squad, plans, training, social/inbox, finances, contracts, personnel and offers |
| `POST /command` | Manager: `{id,day,command}`; host supplies identity and time |
| `GET /wait?after=DAY&after_notice=NOTICE` | Manager: next day, own new transfer request, firing or terminal state |
| `GET /inbox?offset=0&limit=20` | Manager: recipient-scoped messages and exact action/option IDs |
| `GET /staff-market` | Manager: available staff |
| `POST /transfer-market` | Manager: filtered public market facts; detailed scouting is private |
| `GET /public`, `/table`, `/history` | Public state, bounded table, paged full match reports |
| `GET /competitions`, `/team-history` | Public source competition and historical records |
| `GET /news?offset=0&limit=20` | Published factual source news, no future-dated articles |
| `GET /national`, `/world-history` | Bounded national metadata, selected roster/fixtures and archived public facts |
| `GET /player-statistics`, `/team-statistics` | Paged historical match statistics |
| `GET /beat?after=N` | Narrator: retrospective public beat cursor, not raw match cursor |
| `POST /narration` | Narrator: contiguous `{after,through,text}` publication |
| `GET /broadcast` | Public literal narration and coverage offsets |

See the [README](../README.md) for beat and history pagination details.
No private action, tactical intention or conversation enters the narrator feed.
News rumors must remain rumors, not narrated as completed transfers. Missing,
slow or failing narration never controls causal advancement.

An unchanged 25-second long-poll response is not a new model turn. Repeat the
poll inside the tool until its day/notice/beat cursor changes. Firing is permanent;
the other manager, replacement bot and narrator continue.

Manager commands are source-scoped tagged variants: `Training`, `Social`,
`Personnel`, `Economy`, `Market`, `Career`, lineup/squad/match plans and
`Ready`. For example:

```json
{"Career":{"Review":{"action":{"Renew":{"player_id":"PLAYER","weekly_wage":10000,"years":3}}}}}
{"Career":{"Confirm":{"preview_id":12}}}
{"Market":{"Bid":{"player_id":"PLAYER","terms":{"Transfer":{"fee":100000}}}}}
```

Use receipt-provided IDs, never guessed preview IDs. Retries must reuse the exact
request payload; a fresh decision needs a new ID. Scouting and inbox replies
return their required IDs and possible choices.

Trusted stdin operations `init_file`, `save_file`, `load_file` and
`open_window` are not HTTP endpoints or manager tools. File loading avoids
the 8 MiB stdin limit without weakening the bounded command parser. Opening a
fresh window cannot extend a day on which a participant already submitted work.

## Model-free checks

```sh
cargo test --workspace --locked --offline
cargo check --workspace --all-targets --locked --offline
node tools/import-world.test.mjs
node tools/host.test.mjs
node tools/source-text.test.mjs
node tools/replay.test.mjs
```

HTTP tests require ephemeral loopback binding. Replay verifies executable replies
and source state, not model inference or network timing. Generated worlds and
run artifacts are not source code and must not be committed.

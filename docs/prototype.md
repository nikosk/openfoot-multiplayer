# Local league prototype

This is not the requested full OpenFoot parity release. It connects the extracted
engine and implemented management mechanics to real external clients. Using it
for a paid contest requires explicit acceptance of its limits.

## Implemented rules and limitations

- Daily windows close on all-ready or the inclusive deadline; the host advances
  time, not models. Ready managers may respond to existing negotiations until
  closure, but cannot reopen lineup, tactics, recovery or new bids.
- All clubs use the same delegated engine. Managers can set XI, six play styles,
  nine phase dials and Rest/Recovery. Saved instructions persist across days;
  automatic match changes do not replace the saved plan.
- Simple offers require seller preview/confirmation; no wage contract, player
  willingness, loan, registration-window or scouting mechanics are claimed.
  Scheduled leagues reject sales that leave a seller below eleven players.
- Prototype bots rank their own players by OVR times condition, prefer broad
  4-4-2 position counts, select Recovery, reject incoming offers and mark ready.
  They use the same authorized commands. They do not shop, negotiate contracts
  or reproduce OpenFoot's complete management policy. This is a deliberate
  prototype limitation, not a parity claim or permanent bot design.
- Double round robin uses weekly fixture days from day 7, fixed input seed and
  mirrored venues. Both agents' source club is cloned independently into explicit
  league slots; no rating calibration. The normal 20-club case has 380 fixtures,
  ending on day 266. Default daily deadline is 120 seconds; all-ready may finish
  a day sooner. Narration completion is never a prerequisite for advancement.
- Ranking uses points, goal difference and goals scored. Exact ties share the
  prototype championship; ID sorting is only display order.
- Contracts, wages/financial lifecycle, full training, scouting, injuries and
  suspensions, board/firing, cups, promotion/relegation and season rollover are not
  implemented. The source importer rejects injured selected players instead of
  healing them. Existing match events are not promises of persistent injuries.
- Runtime state persists throughout this season; private fsynced host inputs and
  replies can be verified by replay. Automatic crash/resume and multi-season
  continuation are not supported yet. A partial final journal record fails replay
  rather than being silently ignored.

## Build and import

```sh
cargo build -p management --bin league --locked --offline
node tools/import-world.mjs --world /path/to/manifest.json \
  --competition eng-d1 --source-team SOURCE_ID --replace-team OTHER_SLOT_ID \
  --out /path/to/new-private-scenario.json
```

Both slot IDs must be members of the named league. Other participants are retained.
The importer reads immutable referenced shards and records hashes, clone mappings
and explicit unmapped fields. It writes exclusively; it never edits source data.
The two external IDs are `manager:clone-a` and `manager:clone-b`; all others are bots.

## Host and watch

```sh
node tools/host.mjs --bin /absolute/path/to/target/debug/league \
  --scenario /path/to/new-private-scenario.json --out-dir /path/to/new-host-run \
  --port 4319 --day-ms 120000
node tools/watch.mjs http://127.0.0.1:4319
```

Start external clients promptly: the host clock begins on startup. `auth.json`
contains separate random bearer tokens for each external manager and the narrator.
Keep it and `journal.jsonl` private. The output directory must be new. The host
listens only on loopback; this is not an internet-facing authentication service.
SIGINT/SIGTERM stop the child and close resources. A completed host stays available
for spectators until explicitly stopped. Public errors omit private diagnostics;
runtime causes are emitted on the host's private stderr.

## API boundary

| Endpoint | Authority and content |
| --- | --- |
| `GET /observe` | Manager bearer: own squad, plans, offers and upcoming fixtures; no seeds |
| `POST /command` | Manager bearer: `{id, day, command}`; identity/time are host supplied |
| `GET /wait?after=DAY` | Manager bearer: wait for advancement/terminal status, then own observation |
| `GET /public` | Public snapshot, complete results, standings and status |
| `GET /table` | Public club names, standings and at most 20 compact recent reports |
| `GET /history?after=N&limit=N` | Public result-offset pagination, maximum 1,000 results |
| `GET /beat?after=N` | Narrator bearer: completed compact public reports and standings |
| `POST /narration` | Narrator bearer: `{after, through, text}`, contiguous completed-result coverage |
| `GET /broadcast` | Public literal narrative text and coverage offsets |

Long polls return unchanged state after 25 seconds; clients should repeat them
inside the same tool invocation without waking a model. Unknown/future history
cursors reject. Compact reports omit per-touch events and full player-stat maps;
raw public history remains available to other consumers. Narration may lag behind
the game. A blocked or failing narration writer cannot block or fail the game.
Narrative text is untrusted content, not markup or executable instructions.

## Checks (no model calls)

```sh
cargo test --workspace --locked --offline
cargo check --workspace --all-targets --locked --offline
node tools/import-world.test.mjs
node tools/host.test.mjs
node tools/replay.test.mjs
node tools/replay.mjs /path/to/league /path/to/private/journal.jsonl
```

HTTP tests need permission to bind an ephemeral loopback port. These are focused
correctness tests, not a scripted full-season demonstration. Replay compares each
recorded reply against the pinned executable's response; use the original binary
and dependency versions. It does not reproduce model inference or network timing.

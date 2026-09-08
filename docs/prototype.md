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
- Simple paid offers require seller preview/confirmation and retain the player's
  existing contract, subject to buyer wage-budget policy. Player consent on paid
  transfers, loans, registration windows and scouting are not implemented.
  Scheduled leagues reject sales that leave a seller below eleven players.
- Prototype bots rank their own players by OVR times condition, prefer broad
  4-4-2 position counts, select Recovery, reject incoming offers and mark ready.
  They review expected renewal terms within 180 days of expiry and confirm only
  legal accepted previews, respecting let-expire instructions and wage policy.
  They use the same authorized commands. They do not shop
  or reproduce OpenFoot's complete management policy. This is a deliberate
  prototype limitation, not a parity claim or permanent bot design.
- Double round robin uses the imported league start date, weekly fixtures, fixed input seed and
  mirrored venues. Both agents' source club is cloned independently into explicit
  league slots; no rating calibration. The normal 20-club case has 380 fixtures,
  with dates determined by the source calendar. Default daily deadline is 120 seconds; all-ready may finish
  a day sooner. Narration completion is never a prerequisite for advancement.
- Ranking uses points, goal difference and goals scored. Exact ties share the
  prototype championship; ID sorting is only display order.
- Full training, scouting, injuries and
  suspensions, cups, promotion/relegation, retirement and youth intake are not
  implemented. The source importer rejects injured selected players instead of
  healing them. Existing match events are not promises of persistent injuries.
- Contract renewals and free-agent signings have executable acceptance/counter/
  rejection rules, current-state previews and explicit confirmation. Let-expire,
  severance and date-based expiry are consequential. Expiry does not secretly
  renew a player to keep eleven available. Salary `weekly_wage` retains upstream's
  annual-unit behavior; Mondays charge each player/staff wage divided by 52.
  Signed balances preserve debt. Sponsorship and attendance income remain absent.
  Weekly board financial pressure uses that wage-only economy's cash runway,
  alongside debt and wage-budget thresholds, not hypothetical gate income.
- Boards have private objectives, satisfaction and warnings. Dismissal is public,
  permanently ends the original manager's access (including receipt replay), and
  assigns a distinct bot. All managers receive the source external-manager match
  satisfaction rule; this is an explicit multiplayer adaptation. Replacement is
  immediate, not the upstream AI hiring delay.
- Complete seasons archive their final tables/results, pay tier-based prizes,
  update reputation/objectives and generate unique next-season fixtures. Date,
  contracts, money, player condition, plans and dismissed outcomes persist.
  The host defaults to one completed season; `--max-seasons` sets a longer horizon.
  Final spectator standings are the archived finish, not the reset next table.
- Private fsynced host inputs and
  replies can be verified by replay. Trusted local versioned save/load preserves
  simulator state and receipts; this is not automatic host/model crash recovery.
  The host writes `final.checkpoint.json` privately when its configured horizon
  completes. Write failures fail the host rather than pretending state was saved.
  A partial final journal record fails replay
  rather than being silently ignored.

## Build and import

```sh
cargo build -p management --bin league --locked --offline
node tools/import-world.mjs --world /path/to/manifest.json \
  --competition eng-d1 --source-team SOURCE_ID --replace-team OTHER_SLOT_ID --division-tier 0 \
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
| `GET /observe` | Manager bearer: own squad, plans, offers, board, contracts, free agents and upcoming fixtures; no seeds |
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

Fired manager tokens receive `{status:"fired",terminal:true,dismissal:...}` from
observation/wait endpoints and cannot submit commands. The host continues with a
replacement bot; other participants and the narrator are not stopped.

Contract command examples (inside the ordinary `{id,day,command}` envelope):

```json
{"Career":{"Review":{"action":{"Renew":{"player_id":"PLAYER","weekly_wage":10000,"years":3}}}}}
{"Career":{"Confirm":{"preview_id":12}}}
{"Career":{"LetExpire":{"player_id":"PLAYER","enabled":true}}}
```

Use `Sign` with renewal-shaped terms for a free agent or `Terminate` with only
`player_id`. Review may return a counter/rejection instead of a committable preview.
Changed dependencies require another explicit confirmation. All new contract
actions must precede Ready. See [the lifecycle plan](../plans/0008-contracts-board-and-rollover.md).

Trusted JSON-lines process operations `save_file {path}` and `load_file {path}`
write/read private versioned checkpoints; saving never overwrites an existing
file and loading is permitted only before initialization. File operations avoid
the normal 8 MiB command-line limit for full-season report history. These are not
HTTP endpoints or manager tools. `save`/`load {checkpoint}` also support small
in-memory snapshots. Restoring the simulator does not restore model sessions,
host credentials, narration or wall-clock deadlines automatically.

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

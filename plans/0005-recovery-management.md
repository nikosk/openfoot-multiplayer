# Explicit recovery management

Status: implemented limited slice. Full training/development parity remains pending.

## Verified mechanics

Adapt pinned OpenFoot `ofm_core/src/training.rs` rest-day and Recovery-focus
branches, including age, morale, condition, fitness, stamina, average physio
ability and medical-facility factors. Recovery focus also has its existing 5%
fitness nudge; rest consumes no random draw. No invented constant recovery bonus.

The current match registry lacks age, morale and staff records. Require explicit
host-supplied per-player/per-club recovery inputs and seed to enable processing;
do not assume the final career world or create arbitrary invisible defaults.
Host configuration is setup-only, not a manager action. All profiles must match
the registered world and pass validation before accepting configuration.

Managers can choose Rest or Recovery through the authenticated command path, with
the same day/readiness/idempotency rules as lineups. The configured initial mode
is Rest unless the manager selects Recovery. Choices persist across days; physical
changes are staged atomically with day results. Like the pinned process_day path,
this slice applies recovery only on days with no explicit fixture, not an extra
recovery boost on matchdays. Mixed-calendar per-club training is not redesigned here.

Age/morale inputs are fixed scenario metadata for now; full birthdays, conversations,
staff hiring and upgrades must drive them when those systems are integrated. Profiles
travel with players on transfer; club staff/facilities are resolved from new ownership.
Injury branches remain unavailable, not guessed from match commentary.

## Still required for complete training

Other focuses, intensity, weekly schedules, individual/group overrides, potential,
OVR recomputation, coaching specialization, injuries and real staff/facility management.
This release slice must be named recovery management rather than full training.

## Checks

Exact reference arithmetic and boundary factors; invalid input no partial change;
rest no RNG; manager ownership/readiness enforcement; matchdays no double recovery;
off-day recovery persists once; transferred player's new club supplies facilities;
configuration cannot be replaced after day processing begins; existing tests retained.

Verification: all 192 workspace tests pass offline with the lockfile; all-targets
compile check passes; fixtures and transfer examples pass. Imported engine files
remain unchanged. No model-backed run was performed.

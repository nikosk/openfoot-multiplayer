import test from 'node:test';
import assert from 'node:assert/strict';
import { buildScenario } from './import-world.mjs';

function bundle(count = 2) {
  const teams = Array.from({ length: count }, (_, i) => ({ id: `team-${i}`, name: `Team ${i}`, finance: 5000, reputation: 700, wage_budget: 50000, facilities: { medical: 3 } }));
  const attributes = Object.fromEntries('pace stamina strength agility passing shooting tackling dribbling defending positioning vision decisions composure aggression teamwork leadership handling reflexes aerial'.split(' ').map((key, i) => [key, 50 + i]));
  return { manifest: { compatibility: { snapshot_date: '2026-09-08' } }, teams,
    players: teams.flatMap(team => Array.from({ length: 12 }, (_, i) => ({ id: `${team.id}-p${i}`, team_id: team.id, match_name: `Player ${i}`, date_of_birth: '2000-09-09', position: 'Midfielder', attributes: { ...attributes }, ovr: 65, condition: 80, fitness: 70, morale: 60, traits: ['Leader'], injury: null, retired: false, wage: 1040, contract_end: '2028-06-30', market_value: 200000, morale_core: { manager_trust: 50, renewal_state: null } }))),
    staff: [{ id: 'physio', team_id: 'team-0', wage: 520, role: 'Physio', attributes: { physiotherapy: 75 } }, { id: 'coach', team_id: 'team-0', wage: 1040, role: 'Coach', attributes: { physiotherapy: 99 } }],
    competitions: [{ id: 'league', kind: 'League', season: 2026, season_start_month: 8, season_start_day: 1, participant_ids: teams.map(team => team.id) }],
    source: { manifest: { path: '/immutable/world.json', sha256: 'example' } },
  };
}
const options = { competition: 'league', source_team: 'team-0', replace_team: 'team-1', division_tier: 0 };

test('independent equal clones preserve original input and explicit provenance', () => {
  const input = bundle();
  const original = structuredClone(input);
  const result = buildScenario(input, options);
  assert.deepEqual(input, original);
  assert.equal(result.init.players.length, 24);
  assert.deepEqual(result.init.fixtures, []);
  assert.equal(result.init.deadline_ms, 0);
  assert.equal(result.init.require_match_rosters, true);
  assert.equal(result.init.recovery.seed, 1001);
  assert.deepEqual(result.meta.source, input.source);
  for (let i = 0; i < 12; i++) {
    const a = result.init.attributes[i], b = result.init.attributes[12 + i];
    const { id: aid, name: aname, ...adata } = a;
    const { id: bid, name: bname, ...bdata } = b;
    assert.deepEqual(adata, bdata);
    assert.notEqual(aid, bid);
    assert.notEqual(aname, bname);
    assert.notEqual(a.traits, b.traits);
    assert.deepEqual(result.init.recovery.players[aid], { age: 25, morale: 60 });
    assert.deepEqual(result.init.career.contracts[aid], result.init.career.contracts[bid]);
  }
  assert.deepEqual(result.init.recovery.clubs['clone-a'], { medical_level: 3, physiotherapy: [75] });
  result.init.attributes[0].traits.push('changed');
  result.init.recovery.clubs['clone-a'].physiotherapy[0] = 0;
  assert.deepEqual(result.init.attributes[12].traits, ['Leader']);
  assert.deepEqual(result.init.recovery.clubs['clone-b'].physiotherapy, [75]);
  assert.deepEqual(input, original);
});

test('retains all other league clubs and bot identities', () => {
  const result = buildScenario(bundle(20), options);
  assert.equal(result.init.clubs.length, 20);
  assert.equal(result.meta.bot_managers.length, 18);
  assert.deepEqual(result.init.clubs[19], { id: 'team-19', name: 'Team 19', balance: 5000 });
  assert.deepEqual(result.meta.external_managers, ['manager:clone-a', 'manager:clone-b']);
});

test('rejects invalid league membership and unsupported or missing source data', () => {
  for (const changed of [{ replace_team: 'missing' }, { replace_team: 'team-0' }, { competition: 'missing' }]) {
    assert.throws(() => buildScenario(bundle(), { ...options, ...changed }));
  }
  for (const mutate of [
    b => { delete b.players[0].attributes.agility; },
    b => { delete b.players[0].fitness; },
    b => { b.players[0].injury = { name: 'injury' }; },
    b => { b.teams[0].finance = 1.5; },
    b => { delete b.teams[0].wage_budget; },
    b => { b.players[0].contract_end = '2027-02-30'; },
    b => { b.players[0].morale_core.renewal_state = {}; },
    b => { b.teams[0].facilities.medical = 0; },
    b => { b.players[0].date_of_birth = '2000-02-30'; },
    b => { b.players[0].date_of_birth = '1800-01-01'; },
    b => { b.competitions[0].kind = 'Cup'; },
    b => { b.players[0].retired = true; b.players[1].retired = true; },
  ]) {
    const input = bundle(); mutate(input);
    assert.throws(() => buildScenario(input, options));
  }
});

test('career projection preserves debt, equal boards, wages and calendar without extending contracts', () => {
  const input = bundle();
  input.teams[0].finance = -1234;
  input.players[0].contract_end = '2026-09-09';
  const { init } = buildScenario(input, options);
  assert.equal(init.clubs[0].balance, -1234);
  assert.deepEqual(init.boards['manager:clone-a'], init.boards['manager:clone-b']);
  assert.deepEqual(init.career.staff_annual_wages['clone-a'], [520, 1040]);
  assert.equal(init.career.contracts['clone-a:team-0-p0'].end_date, '2026-09-09');
  assert.equal(init.seasons.season_start_month, 8);
  assert.throws(() => buildScenario(input, { ...options, division_tier: undefined }), /division tier/);
});

test('accepts exported manifest timestamps, validates them, and uses their UTC date', () => {
  const input = bundle();
  input.manifest.compatibility.snapshot_date = '2026-06-01T00:00:00+00:00';
  assert.equal(buildScenario(input, options).meta.snapshot_date, '2026-06-01');
  input.manifest.compatibility.snapshot_date = '2026-06-01T00:00:00+03:00';
  assert.equal(buildScenario(input, options).meta.snapshot_date, '2026-05-31');
  for (const invalid of ['2026-02-30T00:00:00Z', '2026-06-01T24:00:00Z', '2026-06-01T00:00:00', 'invalid']) {
    input.manifest.compatibility.snapshot_date = invalid;
    assert.throws(() => buildScenario(input, options));
  }
  assert.throws(() => buildScenario(bundle(), { ...options, date: '2026-06-01T00:00:00Z' }));
  assert.equal(buildScenario(bundle(), { ...options, date: '2026-06-01' }).meta.snapshot_date, '2026-06-01');
});

test('skips retired source players without fabricating or healing their state', () => {
  const input = bundle();
  input.players[0].retired = true;
  input.players[0].injury = { name: 'injury' };
  const result = buildScenario(input, options);
  assert.equal(result.init.players.length, 22);
  assert.ok(result.init.players.every(player => !player.id.endsWith(':team-0-p0')));
});

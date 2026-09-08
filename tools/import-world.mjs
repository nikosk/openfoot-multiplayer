#!/usr/bin/env node
// Explicit prototype projection. This does not convert a complete playable world.
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const ATTRS = 'pace stamina strength agility passing shooting tackling dribbling defending positioning vision decisions composure aggression teamwork leadership handling reflexes aerial'.split(' ');
const text = (value, label) => {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${label} must be nonblank`);
  return value;
};
const integer = (value, label, max = Number.MAX_SAFE_INTEGER, min = 0) => {
  if (!Number.isSafeInteger(value) || value < min || value > max) throw new Error(`${label} must be an integer in ${min}..${max}`);
  return value;
};
function date(value, label) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) throw new Error(`${label} must be YYYY-MM-DD`);
  const parsed = new Date(`${value}T00:00:00Z`);
  if (!Number.isFinite(parsed.valueOf()) || parsed.toISOString().slice(0, 10) !== value) throw new Error(`invalid ${label}`);
  return value;
}
function snapshotDate(value) {
  if (typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value)) return date(value, 'snapshot date');
  const parts = typeof value === 'string' && value.match(/^(\d{4}-\d{2}-\d{2})T([01]\d|2[0-3]):([0-5]\d):([0-5]\d)(?:\.\d+)?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/);
  if (!parts) throw new Error('manifest snapshot date must be a calendar date or ISO timestamp with timezone');
  date(parts[1], 'snapshot date');
  const parsed = new Date(value);
  if (!Number.isFinite(parsed.valueOf())) throw new Error('invalid snapshot timestamp');
  return date(parsed.toISOString().slice(0, 10), 'UTC snapshot date');
}
function index(rows, label) {
  if (!Array.isArray(rows)) throw new Error(`${label} shard must be an array`);
  const result = new Map();
  for (const row of rows) {
    const id = text(row?.id, `${label} ID`);
    if (result.has(id)) throw new Error(`duplicate ${label} ID ${id}`);
    result.set(id, row);
  }
  return result;
}

/** Pure conversion of {manifest, teams, players, staff, competitions, source?}.
 * Options require competition, source_team and replace_team IDs. No source object
 * is mutated and no missing physical attributes are replaced by engine defaults.
 */
export function buildScenario(bundle, options) {
  const teams = index(bundle.teams, 'teams');
  index(bundle.players, 'players');
  index(bundle.staff, 'staff');
  const competitions = index(bundle.competitions, 'competitions');
  const competitionId = text(options.competition, 'competition');
  const sourceId = text(options.source_team, 'source_team');
  const replaceId = text(options.replace_team, 'replace_team');
  const competition = competitions.get(competitionId);
  if (!competition || competition.kind !== 'League') throw new Error('requested competition must be an existing League');
  const members = competition.participant_ids;
  if (!Array.isArray(members) || members.length < 2 || members.length > 128 || new Set(members).size !== members.length) throw new Error('league requires 2..128 distinct participant IDs');
  if (sourceId === replaceId || !members.includes(sourceId) || !members.includes(replaceId)) throw new Error('source and replacement must be different members of the requested league');
  for (const id of members) if (!teams.has(id)) throw new Error(`missing league team ${id}`);
  const snapshot = options.date === undefined
    ? snapshotDate(bundle.manifest?.compatibility?.snapshot_date)
    : date(options.date, 'snapshot date');
  const seed = integer(options.seed ?? 1001, 'seed');
  const deadline = integer(options.deadline_ms ?? 0, 'deadline_ms');
  const init = { op: 'init', clubs: [], players: [], managers: [], attributes: [], fixtures: [], recovery: { seed, players: {}, clubs: {} }, day: 1, deadline_ms: deadline, require_match_rosters: true };
  init.career = { today: snapshot, contracts: {}, wage_budgets: {}, reputations: {}, staff_annual_wages: {} };
  init.boards = {};
  init.seasons = { season: integer(competition.season, 'competition season', 9998, 1),
    season_start_month: integer(competition.season_start_month, 'season start month', 12, 1),
    season_start_day: integer(competition.season_start_day, 'season start day', 31, 1),
    spacing_days: 7, seed, division_tier: integer(options.division_tier, 'division tier', 31) };
  const mappings = { clubs: [], players: [], staff: [] };
  const outputIds = new Set();
  const claim = id => {
    if (outputIds.has(id)) throw new Error(`projected ID collision: ${id}`);
    outputIds.add(id);
  };
  for (const slot of members) {
    const suffix = slot === sourceId ? 'A' : slot === replaceId ? 'B' : null;
    const team = teams.get(suffix ? sourceId : slot);
    const clubId = suffix ? `clone-${suffix.toLowerCase()}` : slot;
    claim(clubId);
    const clubName = text(team.name, 'team name') + (suffix ? suffix === 'A' ? ' North' : ' South' : '');
    init.clubs.push({ id: clubId, name: clubName, balance: integer(team.finance, `${team.id} finance`, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER) });
    const managerId = `manager:${clubId}`;
    claim(managerId);
    init.managers.push({ id: managerId, club_id: clubId });
    const put = (map, key, value) => Object.defineProperty(map, key, { enumerable: true, configurable: true, writable: true, value });
    const reputation = integer(team.reputation, `${team.id} reputation`, 1000);
    put(init.career.reputations, clubId, reputation);
    put(init.career.wage_budgets, clubId, integer(team.wage_budget, `${team.id} wage budget`));
    put(init.career.staff_annual_wages, clubId, bundle.staff.filter(person => person.team_id === team.id)
      .map(person => integer(person.wage, `${person.id} wage`, 4294967295)));
    // Managers are new appointments, not copies of the source human's career.
    put(init.boards, managerId, { reputation, initial_satisfaction: 50 });
    const physios = bundle.staff.filter(person => person.team_id === team.id && person.role === 'Physio');
    Object.defineProperty(init.recovery.clubs, clubId, { enumerable: true, configurable: true, writable: true, value: {
      medical_level: integer(team.facilities?.medical, `${team.id} medical`, 10, 1),
      physiotherapy: physios.map(person => integer(person.attributes?.physiotherapy, `${person.id} physiotherapy`, 100)),
    } });
    if (suffix) {
      mappings.clubs.push({ source_id: sourceId, replaced_slot_id: slot, clone_id: clubId });
      mappings.staff.push(...physios.map(person => ({ source_id: person.id, club_id: clubId, projection: 'physiotherapy rating only' })));
    }
    const players = bundle.players.filter(player => player.team_id === team.id && !player.retired);
    if (players.length < 11) throw new Error(`${team.id} requires at least 11 nonretired players`);
    for (const player of players) {
      if (player.injury != null) throw new Error(`injured player ${player.id}: injury lifecycle is not supported by this prototype`);
      const playerId = suffix ? `${clubId}:${player.id}` : player.id;
      claim(playerId);
      const name = text(player.match_name ?? player.full_name ?? player.name, `${player.id} name`) + (suffix ? ` ${suffix}` : '');
      const position = player.natural_position ?? player.position;
      if (!['Goalkeeper', 'Defender', 'Midfielder', 'Forward'].includes(position)) throw new Error(`invalid position for ${player.id}`);
      if (!Array.isArray(player.traits) || player.traits.some(trait => typeof trait !== 'string')) throw new Error(`missing or invalid traits for ${player.id}`);
      const attributes = Object.fromEntries(ATTRS.map(key => [key, integer(player.attributes?.[key], `${player.id} ${key}`, 100)]));
      init.players.push({ id: playerId, name, club_id: clubId });
      init.attributes.push({ ...attributes, id: playerId, name, position,
        ovr: integer(player.ovr, `${player.id} ovr`, 100),
        condition: integer(player.condition, `${player.id} condition`, 100),
        fitness: integer(player.fitness, `${player.id} fitness`, 100),
        traits: [...player.traits], role: 'Standard' });
      const birth = date(player.date_of_birth, `${player.id} date of birth`);
      if (birth > snapshot) throw new Error(`birth date after snapshot for ${player.id}`);
      const age = Number(snapshot.slice(0, 4)) - Number(birth.slice(0, 4)) - (snapshot.slice(5) < birth.slice(5) ? 1 : 0);
      integer(age, `${player.id} age`, 120);
      if (player.loan != null || player.loan_parent_team_id != null) throw new Error(`loaned player ${player.id}: loan lifecycle is not supported`);
      const core = player.morale_core;
      if (!core || core.renewal_state != null) throw new Error(`${player.id}: missing morale core or active renewal state cannot be projected`);
      put(init.career.contracts, playerId, {
        date_of_birth: birth, weekly_wage: integer(player.wage, `${player.id} wage`, 4294967295),
        end_date: player.contract_end == null ? null : date(player.contract_end, `${player.id} contract end`),
        market_value: integer(player.market_value, `${player.id} market value`),
        morale: integer(player.morale, `${player.id} morale`, 100),
        manager_trust: integer(core.manager_trust, `${player.id} manager trust`, 100),
        unresolved_issue: core.unresolved_issue != null, recent_poor_treatment: core.recent_treatment != null,
        let_expire: false, blocked_until: null, last_attempt: null, last_agreed: null, round: 0,
      });
      Object.defineProperty(init.recovery.players, playerId, { enumerable: true, configurable: true, writable: true, value: { age, morale: integer(player.morale, `${player.id} morale`, 100) } });
      if (suffix) mappings.players.push({ source_id: player.id, clone_id: playerId, club_id: clubId });
    }
  }
  return { init, meta: {
    projection: 'prototype-only; not full world conversion or source outcome parity',
    source: structuredClone(bundle.source ?? {}), competition_id: competitionId, snapshot_date: snapshot,
    clone_mappings: mappings,
    external_managers: ['manager:clone-a', 'manager:clone-b'],
    bot_managers: init.managers.filter(manager => !['manager:clone-a', 'manager:clone-b'].includes(manager.id)).map(manager => manager.id),
    limitations: [
      'Host must set an active deadline and explicitly schedule a new calendar before running.',
      'Source fixtures, standings, history, competition rules and other competitions are not imported.',
      'Contract salary field weekly_wage retains upstream annual-wage semantics; Monday charges divide each wage by 52.',
      'Scouting, training growth, sponsorship, attendance income, loans, retirement and promotion/relegation are not implemented.',
      'Staff identities and behavior are not imported; only Physio physiotherapy ratings affect recovery.',
      'Retired players are skipped; any injured selected player blocks projection. No injury is healed.',
      'Contract negotiation inputs, player/staff wages, budget, reputation and calendar are projected; remaining source data are not.',
      'Managers are new host-controlled identities; source manager configurations and tactics are not imported.',
    ],
  } };
}

async function main(args) {
  const allowed = new Set(['world', 'competition', 'source-team', 'replace-team', 'out', 'seed', 'date', 'deadline-ms', 'division-tier']);
  const flags = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i].startsWith('--') ? args[i].slice(2) : '';
    if (!allowed.has(key) || flags[key] !== undefined || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error(`invalid argument ${args[i]}`);
    flags[key] = args[i + 1];
  }
  for (const key of ['world', 'competition', 'source-team', 'replace-team', 'out', 'division-tier']) text(flags[key], `--${key}`);
  const worldPath = resolve(flags.world);
  const manifestBytes = await readFile(worldPath);
  const manifest = JSON.parse(manifestBytes.toString('utf8'));
  const digest = bytes => createHash('sha256').update(bytes).digest('hex');
  const source = { manifest: { path: worldPath, sha256: digest(manifestBytes) }, shards: {} };
  const bundle = { manifest, source };
  for (const key of ['teams', 'players', 'staff', 'competitions']) {
    const path = resolve(dirname(worldPath), text(manifest.shards?.[key], `${key} shard path`));
    const bytes = await readFile(path);
    source.shards[key] = { path, sha256: digest(bytes) };
    bundle[key] = JSON.parse(bytes.toString('utf8'));
  }
  const result = buildScenario(bundle, { competition: flags.competition, source_team: flags['source-team'], replace_team: flags['replace-team'], date: flags.date,
    division_tier: Number(flags['division-tier']), seed: flags.seed === undefined ? undefined : Number(flags.seed), deadline_ms: flags['deadline-ms'] === undefined ? undefined : Number(flags['deadline-ms']) });
  await writeFile(resolve(flags.out), `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}

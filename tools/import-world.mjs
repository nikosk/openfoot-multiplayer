#!/usr/bin/env node
// Explicit clone scenario import. World scope preserves the exported calendars
// and registries; league scope retains the earlier bounded prototype projection.
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
  init.training = { seed, clubs: {}, players: {} };
  init.availability = {};
  init.squads = { profiles: {}, plans: {} };
  init.social = { seed, source_players: {} };
  init.lineups = {};
  init.personnel = { seed, teams: {}, staff: {} };
  init.economy = { seed, season: integer(competition.season, 'competition season', 9998, 1), clubs: {}, completed_home_dates: {} };
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
  const put = (map, key, value) => Object.defineProperty(map, key, { enumerable: true, configurable: true, writable: true, value });
  function addPlayer(player,clubId,suffix) {
  const playerId = suffix ? `${clubId}:${player.id}` : player.id;
  claim(playerId);
  const name = text(player.match_name ?? player.full_name ?? player.name, `${player.id} name`) + (suffix ? ` ${suffix}` : '');
  const natural = player.natural_position ?? player.position;
  const groups = { Goalkeeper: 'Goalkeeper', Defender: 'Defender', CenterBack: 'Defender', LeftBack: 'Defender', RightBack: 'Defender', LeftWingBack: 'Defender', RightWingBack: 'Defender',
    Midfielder: 'Midfielder', DefensiveMidfielder: 'Midfielder', CentralMidfielder: 'Midfielder', AttackingMidfielder: 'Midfielder', LeftMidfielder: 'Midfielder', RightMidfielder: 'Midfielder',
    Forward: 'Forward', Striker: 'Forward', LeftWinger: 'Forward', RightWinger: 'Forward' };
  const position = groups[natural];
  if (!position || !groups[player.position]) throw new Error(`invalid position for ${player.id}`);
  if (!Array.isArray(player.traits) || player.traits.some(trait => typeof trait !== 'string')) throw new Error(`missing or invalid traits for ${player.id}`);
  const attributes = Object.fromEntries(ATTRS.map(key => [key, integer(player.attributes?.[key], `${player.id} ${key}`, 100)]));
  init.players.push({ id: playerId, name, club_id: clubId });
  const sourcePlayer = structuredClone(player);
  sourcePlayer.id = playerId;
  sourcePlayer.team_id = clubId || null;
  sourcePlayer.match_name = name;
  sourcePlayer.full_name = text(player.full_name ?? player.match_name, `${player.id} full name`) + (suffix ? ` ${suffix}` : '');
  put(init.social.source_players, playerId, sourcePlayer);
  init.attributes.push({ ...attributes, id: playerId, name, position,
    ovr: integer(player.ovr, `${player.id} ovr`, 100),
    condition: integer(player.condition, `${player.id} condition`, 100),
    fitness: integer(player.fitness, `${player.id} fitness`, 100),
    traits: [...player.traits], role: 'Standard' });
  const birth = date(player.date_of_birth, `${player.id} date of birth`);
  if (birth > snapshot) throw new Error(`birth date after snapshot for ${player.id}`);
  const age = Number(snapshot.slice(0, 4)) - Number(birth.slice(0, 4)) - (snapshot.slice(5) < birth.slice(5) ? 1 : 0);
  integer(age, `${player.id} age`, 120);
  put(init.training.players, playerId, { birth_year: Number(birth.slice(0,4)), potential: integer(player.potential, `${player.id} potential`, 100),
    natural_position: natural, position: player.position, individual_focus: player.training_focus ?? null });
  put(init.squads.profiles, playerId, { natural_position: natural, position: player.position,
    alternate_positions: structuredClone(player.alternate_positions ?? []), footedness: text(player.footedness, `${player.id} footedness`),
    weak_foot: integer(player.weak_foot, `${player.id} weak foot`, 5, 1) });
  put(init.availability, playerId, { injury: player.injury == null ? null : { name: text(player.injury.name, `${player.id} injury`), days_remaining: integer(player.injury.days_remaining, `${player.id} injury days`, 4294967295) },
    yellow_cards: integer(player.stats?.yellow_cards ?? 0, `${player.id} yellow cards`, 4294967295), red_cards: integer(player.stats?.red_cards ?? 0, `${player.id} red cards`, 4294967295) });
  if (!worldScope && (player.active_loan != null || player.loan != null || player.loan_parent_team_id != null)) throw new Error(`loaned player ${player.id}: requires world scope`);
  const core = player.morale_core;
  if (!core) throw new Error(`${player.id}: missing morale core`);
  const renewal=core.renewal_state;
  if(renewal && typeof renewal.status!=='string') throw new Error(`${player.id}: malformed renewal state`);
  put(init.career.contracts, playerId, {
    date_of_birth: birth, weekly_wage: integer(player.wage, `${player.id} wage`, 4294967295),
    end_date: player.contract_end == null ? null : date(player.contract_end, `${player.id} contract end`),
    market_value: integer(player.market_value, `${player.id} market value`),
    morale: integer(player.morale, `${player.id} morale`, 100),
    manager_trust: integer(core.manager_trust, `${player.id} manager trust`, 100),
    unresolved_issue: core.unresolved_issue != null, recent_poor_treatment: core.recent_treatment != null,
    let_expire: renewal?.exit_intent === 'LetExpire', blocked_until: renewal?.manager_blocked_until ?? null,
    last_attempt: renewal?.last_attempt_date ?? null, last_agreed: renewal?.status === 'Agreed' ? renewal.last_attempt_date : null,
    round: renewal?.conversation_round ?? 0,
  });
  Object.defineProperty(init.recovery.players, playerId, { enumerable: true, configurable: true, writable: true, value: { age, morale: integer(player.morale, `${player.id} morale`, 100) } });
  if (suffix) mappings.players.push({ source_id: player.id, clone_id: playerId, club_id: clubId });
  }
  const worldScope=options.scope === 'world';
  if(options.scope !== undefined && !['world','league'].includes(options.scope)) throw new Error('scope must be world or league');
  for (const slot of worldScope ? teams.keys() : members) {
    const suffix = slot === sourceId ? 'A' : slot === replaceId ? 'B' : null;
    const team = teams.get(suffix ? sourceId : slot);
    const clubId = suffix ? `clone-${suffix.toLowerCase()}` : slot;
    claim(clubId);
    const clubName = text(team.name, 'team name') + (suffix ? suffix === 'A' ? ' North' : ' South' : '');
    init.clubs.push({ id: clubId, name: clubName, balance: integer(team.finance, `${team.id} finance`, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER) });
    const managerId = `manager:${clubId}`;
    claim(managerId);
    init.managers.push({ id: managerId, club_id: clubId });
    const reputation = integer(team.reputation, `${team.id} reputation`, 1000);
    put(init.career.reputations, clubId, reputation);
    put(init.career.wage_budgets, clubId, integer(team.wage_budget, `${team.id} wage budget`));
    put(init.career.staff_annual_wages, clubId, bundle.staff.filter(person => person.team_id === team.id)
      .map(person => integer(person.wage, `${person.id} wage`, 4294967295)));
    // Managers are new appointments, not copies of the source human's career.
    put(init.boards, managerId, { reputation, initial_satisfaction: 50 });
    const physios = bundle.staff.filter(person => person.team_id === team.id && person.role === 'Physio');
    const remapPlayer = id => id == null ? null : suffix ? `${clubId}:${id}` : id;
    const sourceTeam = structuredClone(team);
    sourceTeam.id = clubId; sourceTeam.name = clubName;
    sourceTeam.starting_xi_ids = (team.starting_xi_ids ?? []).map(remapPlayer);
    sourceTeam.player_roles = Object.fromEntries(Object.entries(team.player_roles ?? {}).map(([id,role]) => [remapPlayer(id),role]));
    sourceTeam.match_roles = Object.fromEntries(Object.entries(team.match_roles ?? {}).map(([key,id]) => [key,remapPlayer(id)]));
    sourceTeam.training_groups = (team.training_groups ?? []).map(group => ({...group,id:suffix?`${clubId}:${group.id}`:group.id,player_ids:group.player_ids.map(remapPlayer)}));
    put(init.personnel.teams, clubId, sourceTeam);
    for (const person of bundle.staff.filter(person => person.team_id === team.id)) {
      const staffId = suffix ? `${clubId}:${person.id}` : person.id;
      claim(staffId);
      put(init.personnel.staff, staffId, {...structuredClone(person),id:staffId,team_id:clubId,
        last_name: `${person.last_name ?? ''}${suffix ? ` ${suffix}` : ''}`});
      if (suffix) mappings.staff.push({source_id:person.id,clone_id:staffId,club_id:clubId});
    }
    put(init.economy.clubs, clubId, {
      wage_budget: integer(team.wage_budget, `${team.id} wage budget`), transfer_budget: integer(team.transfer_budget, `${team.id} transfer budget`, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER),
      season_income: integer(team.season_income, `${team.id} season income`, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER),
      season_expenses: integer(team.season_expenses, `${team.id} season expenses`, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER),
      sponsorship: structuredClone(team.sponsorship ?? null), financial_ledger: structuredClone(team.financial_ledger ?? []),
      reputation, stadium_capacity: integer(team.stadium_capacity, `${team.id} stadium capacity`, 4294967295), form: structuredClone(team.form ?? []),
    });
    put(init.lineups, clubId, (team.starting_xi_ids ?? []).map(remapPlayer));
    const coaches = bundle.staff.filter(person => person.team_id === team.id && ['Coach','AssistantManager'].includes(person.role));
    put(init.training.clubs, clubId, {
      focus: text(team.training_focus, `${team.id} training focus`),
      intensity: text(team.training_intensity, `${team.id} training intensity`),
      schedule: text(team.training_schedule, `${team.id} training schedule`),
      groups: (team.training_groups ?? []).map(group => ({ ...group, id: suffix ? `${clubId}:${group.id}` : group.id, player_ids: group.player_ids.map(remapPlayer) })),
      coaches: coaches.map(person => ({ coaching: integer(person.attributes.coaching, `${person.id} coaching`, 100), specialization: person.specialization ?? null })),
      physiotherapy: physios.map(person => integer(person.attributes?.physiotherapy, `${person.id} physiotherapy`, 100)),
      medical_level: integer(team.facilities?.medical, `${team.id} medical`, 255),
      training_level: integer(team.facilities?.training, `${team.id} training facility`, 255),
    });
    put(init.squads.plans, clubId, { formation: text(team.formation, `${team.id} formation`),
      player_roles: Object.fromEntries(Object.entries(team.player_roles ?? {}).map(([id, role]) => [remapPlayer(id), role])),
      match_roles: Object.fromEntries(['captain','vice_captain','penalty_taker','free_kick_taker','corner_taker'].map(role => [role,remapPlayer(team.match_roles?.[role])])),
    });
    Object.defineProperty(init.recovery.clubs, clubId, { enumerable: true, configurable: true, writable: true, value: {
      medical_level: integer(team.facilities?.medical, `${team.id} medical`, 255),
      physiotherapy: physios.map(person => integer(person.attributes?.physiotherapy, `${person.id} physiotherapy`, 100)),
    } });
    if (suffix) {
      mappings.clubs.push({ source_id: sourceId, replaced_slot_id: slot, clone_id: clubId });
    }
    const players = bundle.players.filter(player => player.team_id === team.id && !player.retired);
    if (players.length < 11) throw new Error(`${team.id} requires at least 11 nonretired players`);
    for (const player of players) addPlayer(player,clubId,suffix);
  }
  if(worldScope) for(const player of bundle.players.filter(player=>!player.team_id)) addPlayer(player,'',null);
  for (const person of bundle.staff.filter(person => person.team_id == null)) {
    claim(person.id);
    Object.defineProperty(init.personnel.staff, person.id, {value:structuredClone(person),enumerable:true,writable:true,configurable:true});
  }
  if(worldScope) {
    delete init.seasons;
    init.retired_player_ids=Object.values(init.social.source_players).filter(player=>player.retired).map(player=>player.id);
    init.match_plans=Object.fromEntries(Object.entries(init.personnel.teams).map(([id,team])=>[id,{play_style:team.play_style,...structuredClone(team.tactics_phase)}]));
    const remapClub=id=>id===sourceId?'clone-a':id===replaceId?'clone-b':id;
    // Replace exact identity values/keys, never substrings in prose or fixture IDs.
    const remap=value=>typeof value==='string'?remapClub(value):Array.isArray(value)?value.map(remap):value&&typeof value==='object'?Object.fromEntries(Object.entries(value).map(([key,item])=>[remapClub(key),remap(item)])):value;
    init.competitions={seed,primary_competition_id:competitionId,
      competitions:Object.fromEntries(bundle.competitions.map(c=>[c.id,remap(c)])),
      competition_order:bundle.competitions.map(c=>c.id),
      active_competition_ids:structuredClone(bundle.manifest.defaultActiveCompetitions),catch_up_past:true,
      club_regions:Object.fromEntries(init.clubs.map(club=>{
        const team=init.personnel.teams[club.id];
        const region=bundle.manifest.regions.find(region=>region.countryCodes.includes(team.football_nation||team.country));
        if(!region) throw new Error(`Missing football region for ${club.id}`);
        return [club.id,region.id];
      }))};
    const sourceManagers=index(bundle.managers,'managers');
    init.team_history={managers:{},actor_manager_ids:{},archived_identities:{
      teams:Object.fromEntries([sourceId,replaceId].map(id=>[id,structuredClone(teams.get(id))])),
      players:Object.fromEntries(bundle.players.filter(player=>[sourceId,replaceId].includes(player.team_id)).map(player=>[player.id,structuredClone(player)])),
      managers:Object.fromEntries(bundle.managers.filter(manager=>[sourceId,replaceId].includes(manager.team_id)).map(manager=>[manager.id,structuredClone(manager)])),
    }};
    for(const manager of sourceManagers.values()) if(!manager.team_id) put(init.team_history.managers,manager.id,structuredClone(manager));
    for(const club of init.clubs) {
      const team=init.personnel.teams[club.id];
      const original=sourceManagers.get(team.manager_id);
      if(!original) throw new Error(`Missing source manager for ${club.id}`);
      const clone=club.id==='clone-a'||club.id==='clone-b';
      const id=clone?`${club.id}:${original.id}`:original.id;
      const manager={...structuredClone(original),id,team_id:club.id};
      if(clone) {
        manager.last_name+=club.id==='clone-a'?' A':' B';
        manager.career_history=manager.career_history.map(entry=>({...entry,team_id:entry.team_id===sourceId?club.id:entry.team_id,team_name:entry.team_id===sourceId?club.name:entry.team_name}));
      }
      team.manager_id=id;
      put(init.team_history.managers,id,manager);
      put(init.team_history.actor_manager_ids,`manager:${club.id}`,id);
      init.boards[`manager:${club.id}`].initial_satisfaction=manager.satisfaction;
    }
    init.market={season_start:`${competition.season}-${String(competition.season_start_month).padStart(2,'0')}-${String(competition.season_start_day).padStart(2,'0')}`};
    init.news={seed,articles:structuredClone(bundle.news ?? []),protected_clubs:['clone-a','clone-b']};
    init.statistics=structuredClone(bundle.stats ?? {player_matches:[],team_matches:[]});
    init.national={seed,national_teams:structuredClone(bundle.nationalTeams),world_history:structuredClone(bundle.worldHistory),
      country_regions:Object.fromEntries(bundle.manifest.regions.flatMap(region=>region.countryCodes.map(code=>[code,region.id])))};
  }
  return { init, meta: {
    projection: worldScope ? 'full-world clone scenario; source calendars and simulation scope retained' : 'bounded league prototype projection',
    source: structuredClone(bundle.source ?? {}), competition_id: competitionId, snapshot_date: snapshot,
    clone_mappings: mappings,
    external_managers: ['manager:clone-a', 'manager:clone-b'],
    bot_managers: init.managers.filter(manager => !['manager:clone-a', 'manager:clone-b'].includes(manager.id)).map(manager => manager.id),
    limitations: [
      ...(worldScope ? [
        'Two league slots are replaced by equal starting-club clones, including their squad/staff/manager identities; this is an explicit experimental world change.',
        'National squads are refreshed from the live post-clone player registry by the source selection rules.',
        'Past scheduled dormant fixtures use explicit source import catch-up; no original artifact is modified.',
      ] : [
        'Host must set an active deadline and explicitly schedule a new calendar before running.',
        'League-only projection omits other competitions, source fixtures, rich managers, free players and national football; do not use for the parity contest.',
        'Retired players are skipped in the legacy league-only projection.',
      ]),
      'Contract salary field weekly_wage retains upstream annual-wage semantics; Monday charges divide each wage by 52.',
      'Shared FIFO transfer consent, actor-scoped decisions and permanent original-manager dismissal are the agreed multiplayer rules.',
    ],
  } };
}

async function main(args) {
  const allowed = new Set(['world', 'competition', 'source-team', 'replace-team', 'out', 'seed', 'date', 'deadline-ms', 'division-tier','scope']);
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
  for (const key of ['teams', 'players', 'staff', 'competitions',...(flags.scope==='world'?['managers','nationalTeams','news','stats','worldHistory']:[])]) {
    const path = resolve(dirname(worldPath), text(manifest.shards?.[key], `${key} shard path`));
    const bytes = await readFile(path);
    source.shards[key] = { path, sha256: digest(bytes) };
    bundle[key] = JSON.parse(bytes.toString('utf8'));
  }
  const result = buildScenario(bundle, { competition: flags.competition, source_team: flags['source-team'], replace_team: flags['replace-team'], date: flags.date,
    scope:flags.scope, division_tier: Number(flags['division-tier']), seed: flags.seed === undefined ? undefined : Number(flags.seed), deadline_ms: flags['deadline-ms'] === undefined ? undefined : Number(flags['deadline-ms']) });
  await writeFile(resolve(flags.out), `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}

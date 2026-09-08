import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { startHost, createSpectatorBeats } from './host.mjs';
import { replayJournal } from './replay.mjs';

function scenario() {
  const clubs = ['a', 'b'].map(id => ({ id, name: `Club ${id}`, balance: 7654321 }));
  const managers = clubs.map(club => ({ id: `manager-${club.id}`, club_id: club.id }));
  const attributes = clubs.flatMap(club => Array.from({ length: 11 }, (_, i) => ({
    ...Object.fromEntries(['ovr', 'fitness', 'pace', 'stamina', 'strength', 'agility', 'passing', 'shooting',
      'tackling', 'dribbling', 'defending', 'positioning', 'vision', 'decisions', 'composure', 'aggression',
      'teamwork', 'leadership', 'handling', 'reflexes', 'aerial'].map(key => [key, 65])),
    id: `${club.id}-${i}`, name: `Player ${club.id}-${i}`, condition: 100,
    position: i === 0 ? 'Goalkeeper' : i < 5 ? 'Defender' : i < 9 ? 'Midfielder' : 'Forward',
    traits: [], role: 'Standard',
  })));
  const players = attributes.map(player => ({ id: player.id, name: player.name, club_id: player.id[0] }));
  return { init: { op: 'init', clubs, managers, attributes, players, recovery: null, day: 1, deadline_ms: 1000,
    fixtures: [{ id: 'final', day: 1, home: 'a', away: 'b', seed: 42 }] },
    meta: { external_managers: managers.map(manager => manager.id), bot_managers: [] } };
}

test('preparation beats are retrospective, weekly, bounded and independent of match cursor', () => {
  const state = createSpectatorBeats();
  const news = [{ id: 'transfer', date: '2026-06-03', headline: 'Completed transfer' },
    { id: 'future', date: '2026-06-09', headline: 'Not yet' }];
  for (let day = 1; day <= 7; day++) state.commit({ day, date: `2026-06-0${day}`, news });
  assert.equal(state.beats.length, 1);
  assert.equal(state.beats[0].kind, 'preparation');
  assert.deepEqual(state.beats[0].results, []);
  assert.deepEqual(state.beats[0].news.map(item => item.id), ['transfer']);
  assert.equal(state.beats[0].from_day, 1);
  assert.equal(state.beats[0].through_day, 7);
  state.commit({ day: 8, date: '2026-06-08', news, terminal: true });
  assert.equal(state.beats.length, 2);
  assert.deepEqual(state.beats[1].news, []);
  const many = createSpectatorBeats();
  many.commit({ day: 1, date: '2026-06-01', terminal: true,
    news: Array.from({length: 45}, (_, i) => ({id: String(i), date: '2026-06-01'})) });
  assert.deepEqual(many.beats.map(beat => beat.news.length), [20, 20, 5]);
});

async function fixture(t, source = scenario(), options = {}) {
  const temp = await mkdtemp(resolve(tmpdir(), 'league-host-test-'));
  const outDir = resolve(temp, 'run');
  const host = await startHost({ bin: resolve('target/debug/league'), scenario: source, outDir, port: 0, dayMs: 60000, ...options });
  t.after(host.close);
  const auth = JSON.parse(await readFile(resolve(outDir, 'auth.json'), 'utf8'));
  async function request(path, token, body) {
    const response = await fetch(host.url + path, { method: body === undefined ? 'GET' : 'POST',
      headers: token ? { authorization: `Bearer ${token}` } : {}, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    return { code: response.status, body: await response.json() };
  }
  return { host, auth, outDir, request };
}

function careerScenario() {
  const source = scenario();
  source.init.fixtures = [
    { id: 'first', day: 1, home: 'a', away: 'b', seed: 42 },
    { id: 'return', day: 2, home: 'b', away: 'a', seed: 43 },
  ];
  source.init.career = { today: '2026-08-01',
    contracts: Object.fromEntries(source.init.players.map(player => [player.id, {
      date_of_birth: '2000-01-01', weekly_wage: 520, end_date: '2028-06-30', market_value: 100000,
      morale: 60, manager_trust: 50, unresolved_issue: false, recent_poor_treatment: false,
      let_expire: false, blocked_until: null, last_attempt: null, last_agreed: null, round: 0,
    }])), wage_budgets: { a: 50000, b: 50000 }, reputations: { a: 700, b: 700 }, staff_annual_wages: {} };
  source.init.boards = Object.fromEntries(source.init.managers.map(manager => [manager.id,
    { reputation: 700, initial_satisfaction: 50 }]));
  source.init.seasons = { season: 2026, season_start_month: 8, season_start_day: 1, spacing_days: 7, seed: 1001, division_tier: 0 };
  return source;
}

async function waitForBotReady(outDir,actor) {
  const until=Date.now()+5000;
  while(Date.now()<until) {
    const rows=(await readFile(resolve(outDir,'journal.jsonl'),'utf8')).trim().split('\n').flatMap(line=>{try{return [JSON.parse(line)];}catch{return [];}});
    if(rows.at(-1)?.input.op==='managers' && rows.some(row=>row.input.op==='command' && row.input.actor===actor && row.output.data?.result?.Ok==='Ready'))return;
    await new Promise(resolveWait=>setTimeout(resolveWait,20));
  }
  assert.fail(`Bot ${actor} did not finish its initial asynchronous decision batch`);
}

test('season completion uses archived final table and keeps private board/contracts out of spectator API', async t => {
  const { auth, request, outDir } = await fixture(t, careerScenario());
  const own = (await request('/observe', auth.managers['manager-a'])).body;
  assert.equal(own.career.date, '2026-08-01');
  assert.equal(Object.keys(own.career.contracts).length, 11);
  assert.ok(own.board);
  for (const day of [1, 2]) {
    const wait = request(`/wait?after=${day}`, auth.managers['manager-a']);
    for (const token of Object.values(auth.managers)) {
      assert.equal((await request('/command', token, { id: `ready-${day}`, day, command: 'Ready' })).code, 200);
    }
    await wait;
  }
  const state = (await request('/public')).body;
  assert.equal(state.status, 'completed');
  assert.equal(state.season_history.length, 1);
  assert.ok(state.standings.every(row => row.played === 2));
  assert.deepEqual(state.standings, state.season_history[0].standings);
  assert.equal(state.season.season, 2027);
  const checkpointPath = resolve(outDir, 'final.checkpoint.json');
  const checkpoint = await readFile(checkpointPath, 'utf8');
  assert.equal(JSON.parse(checkpoint).version, 1);
  assert.equal((await stat(checkpointPath)).mode & 0o777, 0o600);
  assert.ok((await replayJournal(resolve('target/debug/league'), resolve(outDir, 'journal.jsonl'))).verified_entries > 1);
  assert.equal(await readFile(checkpointPath, 'utf8'), checkpoint);
  for (const secret of ['weekly_wage', 'wage_budget', 'satisfaction', 'manager_outcomes', 'reputations', 'seed', 'balance']) {
    assert.ok(!JSON.stringify(state).includes(`"${secret}"`), `Public leak: ${secret}`);
  }
});

test('prototype bot renews an expiring contract through reviewed commands, not privileged expiry repair', async t => {
  const source = careerScenario();
  source.meta.external_managers = ['manager-a'];
  source.meta.bot_managers = ['manager-b'];
  source.init.career.contracts['b-0'].end_date = '2026-08-20';
  const { outDir } = await fixture(t, source);
  await waitForBotReady(outDir,'manager-b');
  const entries = (await readFile(resolve(outDir, 'journal.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  const actions = entries.filter(entry => entry.input.op === 'command' && entry.input.actor === 'manager-b');
  assert.ok(actions.some(entry => entry.input.request.command?.Career?.Review));
  assert.ok(actions.some(entry => entry.output.data?.result?.Ok?.Career?.Applied?.action?.Renew));
});

test('dismissal revokes old token commands and waits while a replacement bot keeps the club active', async t => {
  const source = scenario();
  source.init.fixtures[0].day = 5;
  source.init.boards = { 'manager-a': { reputation: 700, initial_satisfaction: 10 },
    'manager-b': { reputation: 700, initial_satisfaction: 50 } };
  const { auth, request } = await fixture(t, source);
  for (const day of [1, 2]) {
    const wait = request(`/wait?after=${day}`, auth.managers['manager-a']);
    for (const token of Object.values(auth.managers)) {
      assert.equal((await request('/command', token, { id: `ready-${day}`, day, command: 'Ready' })).code, 200);
    }
    const result = await wait;
    assert.equal(result.body.status, day === 2 ? 'fired' : 'running');
  }
  const old = (await request('/observe', auth.managers['manager-a'])).body;
  assert.equal(old.status, 'fired');
  assert.equal(old.manager_view, undefined);
  assert.equal((await request('/command', auth.managers['manager-a'], { id: 'ready-1', day: 1, command: 'Ready' })).code, 403);
  const state = (await request('/public')).body;
  assert.equal(state.dismissals.length, 1);
  assert.equal(state.active_managers, undefined);
  const wait = request('/wait?after=3', auth.managers['manager-b']);
  assert.equal((await request('/command', auth.managers['manager-b'], { id: 'ready-3', day: 3, command: 'Ready' })).code, 200);
  assert.equal((await wait).body.observation.day, 4);
});

test('manager tokens scope commands and observations; public endpoints contain no private state', async t => {
  const { auth, request, outDir } = await fixture(t);
  assert.equal((await stat(resolve(outDir, 'auth.json'))).mode & 0o777, 0o600);
  assert.equal((await request('/observe')).code, 401);
  assert.equal((await request('/observe', auth.narrator)).code, 401);
  for (const path of ['/inbox','/staff-market','/transfer-market']) {
    assert.equal((await request(path,auth.narrator,path==='/transfer-market'?{}:undefined)).code,401);
    assert.equal((await request(path,undefined,path==='/transfer-market'?{}:undefined)).code,401);
  }
  const own = await request('/observe?actor=manager-b', auth.managers['manager-a']);
  assert.equal(own.body.manager_view.club.id, 'a');
  assert.equal(own.body.squad.length, 11);
  assert.ok(own.body.squad.every(player => player.id.startsWith('a-')));
  assert.ok(!JSON.stringify(own.body.fixtures).includes('seed'));
  assert.equal((await request('/command', auth.managers['manager-a'], { actor: 'manager-b', id: 'hack', day: 1, command: 'Ready' })).code, 400);
  assert.equal((await request('/command', 'outsider', { id: 'hack', day: 1, command: 'Ready' })).code, 401);
  const crossClub = await request('/command', auth.managers['manager-a'], { id: 'foreign-lineup', day: 1,
    command: { SetLineup: { player_ids: Array.from({ length: 11 }, (_, index) => `b-${index}`) } } });
  assert.equal(crossClub.code, 400);
  assert.ok(crossClub.body.error);
  assert.equal(crossClub.body.receipt.result.Err, crossClub.body.error);
  const publicState = await request('/public');
  for (const secret of ['7654321', 'balance', 'match_plan', 'recovery_view', ...Object.values(auth.managers), auth.narrator]) {
    assert.ok(!JSON.stringify(publicState.body).includes(secret), `Public leak: ${secret}`);
  }
  assert.equal(publicState.body.status, 'running');
  const table = (await request('/table')).body;
  assert.equal(table.day, 1); assert.equal(table.clubs.length, 2);
  assert.deepEqual(table.last_results, []);
  assert.ok(!JSON.stringify(table).includes('balance'));
  const journal = await readFile(resolve(outDir, 'journal.jsonl'), 'utf8');
  assert.ok(journal.split('\n').filter(Boolean).every(line => { const entry = JSON.parse(line); return entry.input && entry.output; }));
  assert.equal((await request('/beat?after=9', auth.narrator)).code, 400);
});

test('ready manager wakes for its own same-day offer without notifying an unrelated club',async t=>{
  const source=scenario();
  source.init.clubs.push({id:'c',name:'Club c',balance:1000});
  source.init.managers.push({id:'manager-c',club_id:'c'});
  for(const player of source.init.players.filter(player=>player.club_id==='b')) source.init.players.push({...player,id:player.id.replace('b-','c-'),club_id:'c'});
  for(const player of source.init.attributes.filter(player=>player.id.startsWith('b-'))) source.init.attributes.push({...structuredClone(player),id:player.id.replace('b-','c-')});
  source.meta.external_managers.push('manager-c');
  const {auth,request}=await fixture(t,source);
  const a=auth.managers['manager-a'],b=auth.managers['manager-b'],c=auth.managers['manager-c'];
  const observed=(await request('/observe',a)).body;
  assert.equal(observed.transfer_notice,0);
  assert.equal((await request('/command',a,{id:'ready',day:1,command:'Ready'})).code,200);
  const waiting=request('/wait?after=1&after_notice=0',a);
  const bid={id:'bid',day:1,command:{Offer:{player_id:'a-0',fee:100}}};
  assert.equal((await request('/command',b,bid)).code,200);
  const changed=(await waiting).body;
  assert.equal(changed.observation.day,1);
  assert.equal(changed.transfer_notice,1);
  assert.equal(changed.observation.transfer_notice,1);
  assert.equal((await request('/observe',c)).body.transfer_notice,0);
  assert.equal((await request('/command',b,bid)).code,200);
  assert.equal((await request('/observe',a)).body.transfer_notice,1,'Replay must not invent another transfer notification');
});

test('all-ready advances and releases manager/narrator waits without narration blocking completion', async t => {
  const { auth, request } = await fixture(t);
  const waiting = request('/wait?after=1', auth.managers['manager-a']);
  const beat = request('/beat?after=0', auth.narrator);
  for (const token of Object.values(auth.managers)) {
    const response = await request('/command', token, { id: 'ready', day: 1, command: 'Ready' });
    assert.equal(response.body.result.Ok, 'Ready');
  }
  const advanced = await waiting;
  assert.equal(advanced.body.status, 'completed');
  assert.equal(advanced.body.observation.day, 2);
  const events = (await beat).body;
  assert.equal(events.next, 1); assert.equal(events.results.length, 1);
  assert.equal(events.clubs.length, 2);
  assert.ok(!('events' in events.results[0].report));
  assert.ok(!('player_stats' in events.results[0].report));
  assert.ok(Array.isArray(events.results[0].report.goals));
  assert.equal(events.status, 'completed');
  const final = (await request('/public')).body;
  assert.equal(final.status, 'completed'); assert.deepEqual(final.champions, [final.standings[0].club_id]);
  assert.ok(Array.isArray(final.results[0].report.events));
  const table = (await request('/table')).body;
  assert.deepEqual(table.last_results, events.results);
  assert.equal((await request('/broadcast')).body.narrations.length, 0);
  assert.equal((await request('/narration', auth.managers['manager-a'], { after: 0, through: 1, text: 'No' })).code, 401);
  assert.equal((await request('/narration', auth.narrator, { after: 0, through: 2, text: 'Future' })).code, 400);
  const item = { after: 0, through: 1, text: '<script>literal text</script>' };
  assert.equal((await request('/narration', auth.narrator, item)).body.next, 1);
  assert.equal((await request('/narration', auth.narrator, item)).code, 400);
  assert.deepEqual((await request('/broadcast')).body.narrations, [item]);
  assert.equal((await request('/command', auth.managers['manager-a'], { id: 'late', day: 2, command: 'Ready' })).code, 409);
});

test('HTTP preparation beat can be narrated before any match while history remains a match cursor', async t => {
  const source = scenario();
  source.init.fixtures[0].day = 9;
  const { auth, request } = await fixture(t, source);
  for (let day = 1; day <= 7; day++) {
    const waiting = request(`/wait?after=${day}`, auth.managers['manager-a']);
    for (const token of Object.values(auth.managers)) {
      assert.equal((await request('/command', token, { id: `ready-${day}`, day, command: 'Ready' })).code, 200);
    }
    await waiting;
  }
  const beat = (await request('/beat?after=0', auth.narrator)).body;
  assert.equal(beat.cursor_kind, 'public_beats');
  assert.equal(beat.next, 1);
  assert.equal(beat.beats[0].kind, 'preparation');
  assert.equal(beat.beats[0].through_day, 7);
  assert.deepEqual(beat.results, []);
  assert.equal(beat.status, 'running');
  assert.equal((await request('/history?after=0')).body.next, 0);
  assert.equal((await request('/narration', auth.narrator, { after: 0, through: 1,
    text: 'Days 1–7 have closed; no matches have finished.' })).code, 200);
});

test('prototype bot readies its club through the same command journal', async t => {
  const source = scenario(); source.meta.external_managers = ['manager-a']; source.meta.bot_managers = ['manager-b'];
  const { auth, request, outDir } = await fixture(t, source);
  await waitForBotReady(outDir,'manager-b');
  assert.deepEqual(Object.keys(auth.managers), ['manager-a']);
  const beforeIdle = await readFile(resolve(outDir, 'journal.jsonl'), 'utf8');
  await new Promise(resolveWait => setTimeout(resolveWait, 550));
  assert.equal(await readFile(resolve(outDir, 'journal.jsonl'), 'utf8'), beforeIdle, 'Idle clock must not journal repeated full bot observations');
  const offerRequest = { id: 'offer', day: 1, command: { Offer: { player_id: 'b-1', fee: 100 } } };
  assert.equal((await request('/command', auth.managers['manager-a'], offerRequest)).code, 200);
  await new Promise(resolveWait => setTimeout(resolveWait, 350));
  const observation = (await request('/observe', auth.managers['manager-a'])).body;
  assert.equal(observation.manager_view.offers[0].status, 'Rejected');
  const waiting = request('/wait?after=1', auth.managers['manager-a']);
  await request('/command', auth.managers['manager-a'], { id: 'ready', day: 1, command: 'Ready' });
  assert.equal((await waiting).body.status, 'completed');
  const entries = (await readFile(resolve(outDir, 'journal.jsonl'), 'utf8')).trim().split('\n').map(JSON.parse);
  assert.ok(entries.some(entry => entry.input.actor === 'manager-b' && entry.input.request?.command === 'Ready'));
});

test('stalled and failing narration storage cannot block or fail the game', async t => {
  const source = scenario();
  source.init.fixtures.push({ id: 'return', day: 2, home: 'b', away: 'a', seed: 43 });
  let releaseWriter, writerStarted;
  const writerGate = new Promise(resolveWriter => { releaseWriter = resolveWriter; });
  const started = new Promise(resolveStarted => { writerStarted = resolveStarted; });
  t.after(() => releaseWriter());
  const { auth, request } = await fixture(t, source, { writeBroadcast: async () => {
    writerStarted(); await writerGate; throw new Error('Injected broadcast disk failure');
  } });
  for (const token of Object.values(auth.managers)) await request('/command', token, { id: 'ready-1', day: 1, command: 'Ready' });
  assert.equal((await request('/wait?after=1', auth.managers['manager-a'])).body.status, 'running');
  const narration = request('/narration', auth.narrator, { after: 0, through: 1, text: 'First fixture' });
  await started;
  for (const token of Object.values(auth.managers)) await request('/command', token, { id: 'ready-2', day: 2, command: 'Ready' });
  assert.equal((await request('/wait?after=2', auth.managers['manager-a'])).body.status, 'completed');
  releaseWriter();
  assert.equal((await narration).code, 503);
  assert.equal((await request('/public')).body.status, 'completed');
  assert.deepEqual((await request('/broadcast')).body.narrations, []);
});

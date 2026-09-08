#!/usr/bin/env node
// Trusted local orchestration. Tokens grant one fixed manager identity or the
// read-only narrator role. Bot policy is deliberately a prototype: rank fit
// players by OVR * condition, prefer 4-4-2, recover, reject offers, then ready.
// The private journal is evidence, not a supported resume/checkpoint format.
import http from 'node:http';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { randomBytes } from 'node:crypto';
import { mkdir, open, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { renderSourceText } from './source-text.mjs';

async function appendBroadcast(file, item) {
  await file.writeFile(`${JSON.stringify(item)}\n`);
  await file.sync();
}

// Model-facing table/beat summaries preserve scores, goals and aggregate facts;
// touch-by-touch events and individual stat maps remain in /public and /history.
function compactMatch(match) {
  const { fixture_id, day, home, away, home_starting_xi, away_starting_xi } = match;
  const { home_goals, away_goals, goals, home_stats, away_stats, home_possession, total_minutes } = match.report;
  return { fixture_id, day, home, away, home_starting_xi, away_starting_xi,
    report: { home_goals, away_goals, goals, home_stats, away_stats, home_possession, total_minutes } };
}

// A beat cursor counts committed public chunks, never raw fixtures. Quiet
// preparation accumulates until seven closed days; matches and terminal state
// flush it sooner. No manager commands or private observations enter this state.
export function createSpectatorBeats() {
  const beats = [], newsIds = new Set(), dismissalIds = new Set();
  let resultOffset = 0, lastCommit, pending = { from_day: null, through_day: null, from_date: null, through_date: null, news: [], events: [] };
  return {
    beats,
    flush() { if (lastCommit && pending.from_day !== null) this.commit({ ...lastCommit, terminal: true }); },
    commit({ day, date = null, results = [], news = [], dismissals = [], standings = [], terminal = false }) {
      lastCommit = { day, date, results, news, dismissals, standings };
      pending.from_day ??= day; pending.from_date ??= date;
      pending.through_day = day; pending.through_date = date;
      for (const article of news) {
        const articleDate = article.date?.slice(0, 10);
        if (!newsIds.has(article.id) && (!date || articleDate <= date)) {
          newsIds.add(article.id); pending.news.push(article);
        }
      }
      for (const item of dismissals) {
        const id = `${item.manager_id}:${item.day}`;
        if (!dismissalIds.has(id)) { dismissalIds.add(id); pending.events.push({ type: 'manager_dismissal', ...item }); }
      }
      const matches = results.slice(resultOffset).map(compactMatch);
      resultOffset = results.length;
      if (!matches.length && !terminal && day - pending.from_day < 6) return;
      const kind = matches.length ? 'matches' : 'preparation';
      // Bound each model response even on a full-world matchday/news backlog.
      const parts = Math.max(1, Math.ceil(matches.length / 20), Math.ceil(pending.news.length / 20), Math.ceil(pending.events.length / 20));
      for (let part = 0; part < parts; part++) beats.push({
        kind, from_day: pending.from_day, through_day: day,
        from_date: pending.from_date, through_date: date,
        standings: structuredClone(standings),
        results: matches.slice(part * 20, (part + 1) * 20),
        news: pending.news.slice(part * 20, (part + 1) * 20),
        events: pending.events.slice(part * 20, (part + 1) * 20),
      });
      pending = { from_day: null, through_day: null, from_date: null, through_date: null, news: [], events: [] };
    },
  };
}

export async function startHost({ bin, scenario, outDir, port = 4319, dayMs = 120000, maxSeasons = 1,
  writeBroadcast = appendBroadcast }) {
  if (typeof bin !== 'string' || !bin || typeof outDir !== 'string' || !outDir) throw new Error('--bin and --out-dir are required');
  if (!Number.isSafeInteger(dayMs) || dayMs < 1) throw new Error('Invalid day duration');
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('Invalid port');
  if (!Number.isSafeInteger(maxSeasons) || maxSeasons < 1) throw new Error('Invalid season limit');
  const source = typeof scenario === 'string' ? JSON.parse(await readFile(scenario, 'utf8')) : structuredClone(scenario);
  const { init, meta } = source;
  const external = meta.external_managers;
  let bots = meta.bot_managers;
  let managers = init.managers.map(manager => manager.id);
  if (![external, bots].every(Array.isArray) || new Set([...external, ...bots]).size !== managers.length ||
      external.length + bots.length !== managers.length || managers.some(id => ![...external, ...bots].includes(id))) {
    throw new Error('Scenario manager roles must partition the manager registry');
  }
  await mkdir(outDir, { mode: 0o700 }); // Fresh directory required; never overwrite evidence.
  const credentials = { managers: Object.fromEntries(external.map(id => [id, randomBytes(32).toString('hex')])), narrator: randomBytes(32).toString('hex') };
  let authFile, journal, broadcastFile, child;
  try {
    authFile = await open(resolve(outDir, 'auth.json'), 'wx', 0o600);
    await authFile.writeFile(JSON.stringify(credentials, null, 2)); await authFile.sync(); await authFile.close();
    authFile = undefined;
    journal = await open(resolve(outDir, 'journal.jsonl'), 'wx', 0o600);
    broadcastFile = await open(resolve(outDir, 'broadcast.jsonl'), 'wx', 0o600);
    child = spawn(bin, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  } catch (error) {
    child?.kill();
    await Promise.allSettled([authFile, journal, broadcastFile].filter(Boolean).map(file => file.close()));
    throw error;
  }
  let childStderr = Buffer.alloc(0);
  child.stderr.on('data', chunk => {
    childStderr = Buffer.concat([childStderr, chunk]).subarray(-16384);
  });
  let pending;
  let rpcChain = Promise.resolve();
  let mutationChain = Promise.resolve();
  let narrationChain = Promise.resolve();
  let status = 'running';
  let failure;
  let timer;
  let closed = false;
  let snapshot = { state: { day: init.day, clubs: [], players: [] }, standings: [], results: [] };
  // Initialization and causal day processing are outside the decision window.
  // The trusted open_window operation starts wall time before participants act.
  let deadline = Number.MAX_SAFE_INTEGER;
  let lastDay = init.day;
  let ready = new Set();
  let botDay;
  let botOffersDirty = false;
  let lastOfferedSequence = 0;
  const transferNotices=new Map(external.map(actor=>[actor,0]));
  const externalClubs=new Map(init.managers.filter(manager=>external.includes(manager.id)).map(manager=>[manager.id,manager.club_id]));
  let advancing = false;
  const narrations = [];
  const spectator = createSpectatorBeats();
  const waiters = new Set();
  const wake = () => { for (const callback of [...waiters]) callback(); };
  function fail(error) {
    if (closed || status === 'failed') return;
    status = 'failed'; failure = 'Host execution failed'; clearInterval(timer);
    spectator.flush();
    // Private process stderr retains the cause; public endpoints get only the
    // generic status above. Bound both fields to avoid runaway diagnostic logs.
    console.error(JSON.stringify({ event: 'host_failure', error: String(error?.stack ?? error).slice(0, 4096),
      child_stderr: childStderr.toString('utf8') }));
    pending?.reject(error); pending = undefined; wake();
    child.kill();
  }
  child.on('error', fail);
  child.on('exit', (code, signal) => { if (!closed) fail(new Error(`Game process exited (code=${code}, signal=${signal})`)); });
  child.stdin.on('error', fail);
  createInterface({ input: child.stdout }).on('line', line => {
    const active = pending; pending = undefined;
    if (!active) return fail(new Error('Unsolicited game output'));
    try { active.resolve(JSON.parse(line)); } catch (error) { active.reject(error); fail(error); }
  });
  async function record(file, value) {
    try { await file.writeFile(`${JSON.stringify(value)}\n`); await file.sync(); }
    catch (error) { fail(error); throw error; }
  }
  function rpc(input) {
    const operation = rpcChain.then(async () => {
      if (closed || status === 'failed') throw new Error('Host unavailable');
      const output = await new Promise((resolveResponse, reject) => {
        const timeout = setTimeout(() => { const error = new Error('Game response timeout'); fail(error); reject(error); }, 30000);
        pending = { resolve: value => { clearTimeout(timeout); resolveResponse(value); }, reject: error => { clearTimeout(timeout); reject(error); } };
        child.stdin.write(`${JSON.stringify(input)}\n`);
      });
      await record(journal, { input, output });
      return output;
    });
    rpcChain = operation.catch(() => {});
    return operation;
  }
  async function required(input) {
    const output = await rpc(input);
    if (!output.ok) throw new Error(output.error);
    return ['observe','inbox','command','transfer_market','staff_market','news'].includes(input.op)
      ? renderSourceText(output.data) : output.data;
  }
  const mutate = task => {
    const operation = mutationChain.then(task);
    mutationChain = operation.catch(() => {});
    return operation;
  };
  const table = () => status === 'completed' && snapshot.season_history?.length
    ? snapshot.season_history.at(-1).standings : snapshot.standings;
  const dismissal = actor => snapshot.dismissals?.find(item => item.manager_id === actor);
  const publicView = () => {
    const standings = table();
    const leader = standings[0];
    const champions = status === 'completed' && leader ? [leader.club_id] : [];
    return { ...snapshot, standings, status, deadline_ms: deadline, last_day: lastDay, champions, ...(failure ? { error: failure } : {}) };
  };
  async function refresh() {
    snapshot = await required({ op: 'public' });
    // Manager routing is trusted-host metadata, not a spectator roster.
    const registry = await required({ op: 'managers' });
    managers = registry.active_managers.map(manager => manager.id);
    bots = managers.filter(actor => !external.includes(actor));
    ready = new Set([...ready].filter(actor => managers.includes(actor)));
  }
  async function command(actor, request, { refreshAfter = true } = {}) {
    const result = await rpc({ op: 'command', actor, request, now_ms: Date.now() });
    if (result.ok && result.data.result?.Ok === 'Ready') ready.add(actor);
    const outcome = result.data?.result?.Ok;
    if (result.ok && (outcome?.Offered || outcome?.Market?.Offer || outcome?.Market?.AwaitingConsent) && result.data.sequence > lastOfferedSequence) {
      lastOfferedSequence = result.data.sequence;
      botOffersDirty = true;
      const offer=outcome.Offered ?? outcome.Market?.Offer ?? outcome.Market?.AwaitingConsent;
      for(const [recipient,club] of externalClubs) {
        if(recipient!==actor && [offer.buyer,offer.seller].includes(club)) transferNotices.set(recipient,transferNotices.get(recipient)+1);
      }
      wake();
    }
    if (refreshAfter) await refresh();
    return result;
  }
  const botSerials = new Map();
  async function botAction(actor, day, action) {
    if (status !== 'running' || snapshot.state.day !== day || Date.now() >= deadline || !managers.includes(actor)) return null;
    const key = `${day}:${actor}`;
    const serial = (botSerials.get(key) ?? 0) + 1;
    botSerials.set(key, serial);
    return mutate(()=>command(actor, { id: `bot-${day}-${serial}`, day, command: action }, { refreshAfter: false }));
  }
  async function executeBotAction(actor, day, action) {
    // Preview references are receipt data, never guessed. Changed dependencies
    // require a fresh receipt; cap retries so contention cannot delay the clock.
    for (let attempt = 0; attempt < 3; attempt++) {
      const response = await botAction(actor, day, action);
      if (!response?.ok) return;
      const outcome = response.data?.result?.Ok;
      if (!outcome) return; // ordinary refusal, not a host/league failure
      const career = outcome.Career?.Preview ?? outcome.Career?.RefreshRequired;
      const market = outcome.Market?.Preview ?? outcome.Market?.Refreshed;
      const personnel = outcome.Personnel;
      if (career) action = { Career: { Confirm: { preview_id: career.id } } };
      else if (market) action = { Market: { Confirm: { preview_id: market.id } } };
      else if (personnel?.preview_id !== undefined) action = { Personnel: { Confirm: { preview_id: personnel.preview_id } } };
      else return;
    }
  }
  async function executeBotPlan(actor, responsesOnly, preparationOnly = false) {
    const day = snapshot.state.day;
    if (Date.now() >= deadline || !managers.includes(actor)) return;
    const response = await rpc({ op: 'bot_plan', actor, responses_only: responsesOnly, preparation_only: preparationOnly });
    if (!response.ok) return; // manager eliminated/unavailable policy: keep clock moving
    const actions = response.data?.commands;
    if (!Array.isArray(actions)) return;
    const selectedActions = preparationOnly ? actions.filter(action => action?.SetLineup || action?.Training || action?.SetRecovery) : actions;
    for (const action of selectedActions.slice(0, 96)) {
      if (Date.now() >= deadline) break;
      await executeBotAction(actor, day, action);
    }
  }
  async function runBotResponses() {
    // Event-driven, bounded rounds, not an every-250ms full-squad observation.
    // A human can leave an offer unanswered; neither bots nor offers add grace.
    for (let round = 0; round < 3 && botOffersDirty && Date.now() < deadline; round++) {
      botOffersDirty = false;
      for (const actor of bots) await executeBotPlan(actor, true);
    }
    botOffersDirty = false;
    await refresh();
  }
  async function runBots() {
    const day = snapshot.state.day;
    if (botDay === day) return;
    botDay = day;
    botSerials.clear();
    for (const actor of bots) {
      await executeBotPlan(actor, false);
    }
    await runBotResponses();
    // Transfers and loans can change squads during the shared response pass.
    // Re-observe preparation inputs before locking in the day's own lineup.
    for (const actor of bots) await executeBotPlan(actor, false, true);
    // Mark ready only after the ordinary response pass; all-ready closure still
    // wins over later responses, including those an external actor was writing.
    for (const actor of bots) await botAction(actor, day, 'Ready');
    await refresh();
  }
  async function advance() {
    if (status !== 'running') return;
    await runBots();
    if (botOffersDirty) {
      await runBotResponses();
    }
    await mutate(async()=>{
    if (Date.now() < deadline && ready.size !== managers.length) return;
    const day = snapshot.state.day;
    const now = Date.now();
    deadline = Number.MAX_SAFE_INTEGER;
    await required({ op: 'tick', day, now_ms: now, next_deadline_ms: deadline });
    ready = new Set();
    await refresh();
    if (snapshot.season_history?.length >= maxSeasons || (!init.seasons && !init.competitions && day >= lastDay)) {
      await required({ op: 'save_file', path: resolve(outDir, 'final.checkpoint.json') });
      status = 'completed'; clearInterval(timer);
    }
    const date = init.career?.today ? new Date(Date.parse(`${init.career.today}T00:00:00Z`) + (day - 1) * 86400000).toISOString().slice(0, 10) : null;
    const news = [];
    if (init.news) {
      for (let offset = 0; ; offset += 100) {
        const { articles: page } = await required({ op: 'news', offset, limit: 100 });
        // Public news may include dated imports. Only this closed interval is
        // retrospective, not tomorrow's now-visible career-clock articles.
        news.push(...page.filter(article => article.date?.slice(0, 10) >= init.career.today));
        if (page.length < 100) break;
      }
    }
    spectator.commit({ day, date, results: snapshot.results, news, dismissals: snapshot.dismissals ?? [], standings: table(), terminal: status !== 'running' });
    if(status==='running') {
      const opened=Date.now();deadline=opened+dayMs;
      await required({op:'open_window',day:snapshot.state.day,now_ms:opened,deadline_ms:deadline});
    }
    wake();
    });
  }
  function waitFor(req, res, predicate) {
    if (predicate()) return Promise.resolve();
    return new Promise(resolveWait => {
      const done = () => { clearTimeout(timeout); waiters.delete(check); res.off('close', done); resolveWait(); };
      const check = () => { if (predicate()) done(); };
      const timeout = setTimeout(done, 25000);
      res.once('close', done); waiters.add(check); check();
    });
  }
  function token(req) { return req.headers.authorization?.match(/^Bearer ([A-Za-z0-9]+)$/)?.[1]; }
  function actorFor(req) { return external.find(actor => credentials.managers[actor] === token(req)); }
  async function body(req) {
    let size = 0; const chunks = [];
    for await (const chunk of req) {
      size += chunk.length;
      if (size > 65536) throw Object.assign(new Error('Body exceeds 64 KiB'), { statusCode: 413 });
      chunks.push(chunk);
    }
    try { return JSON.parse(Buffer.concat(chunks).toString('utf8')); }
    catch { throw Object.assign(new Error('Invalid JSON'), { statusCode: 400 }); }
  }
  const number = (url, key, fallback) => {
    const raw = url.searchParams.get(key);
    if (raw === null) return fallback;
    if (!/^\d+$/.test(raw) || !Number.isSafeInteger(Number(raw))) throw Object.assign(new Error('Invalid cursor'), { statusCode: 400 });
    return Number(raw);
  };
  const send = (res, code, value) => {
    if (res.destroyed) return;
    res.writeHead(code, { 'content-type': 'application/json', 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' });
    res.end(JSON.stringify(value));
  };
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost');
      const actor = actorFor(req);
      if (req.method === 'GET' && url.pathname === '/public') return send(res, 200, publicView());
      if (req.method === 'GET' && url.pathname === '/news') return send(res,200,await required({op:'news',offset:number(url,'offset',0),limit:number(url,'limit',20)}));
      if (req.method === 'GET' && ['/player-statistics','/team-statistics'].includes(url.pathname)) {
        const player = url.pathname === '/player-statistics';
        const key = player ? 'player_id' : 'club_id';
        const id = url.searchParams.get(key);
        if (!id || id.length > 256) return send(res, 400, { error: `Missing or invalid ${key}` });
        return send(res, 200, await required({ op: player ? 'player_statistics' : 'team_statistics',
          [key]: id, offset: number(url, 'offset', 0), limit: Math.min(number(url, 'limit', 20), 100) }));
      }
      if (req.method === 'GET' && url.pathname === '/competitions') return send(res,200,await required({op:'competitions'}));
      if (req.method === 'GET' && url.pathname === '/national') return send(res,200,await required({
        op:'national',nation_id:url.searchParams.get('nation_id'),offset:number(url,'offset',0),limit:Math.min(number(url,'limit',20),100)}));
      if (req.method === 'GET' && url.pathname === '/world-history') {
        const category=url.searchParams.get('category');
        if (!['rivalries','season_awards','world_cup_champions','national_team_ranking','world_cup_hosts'].includes(category)) return send(res,400,{error:'Invalid world-history category'});
        return send(res,200,await required({op:'world_history',category,offset:number(url,'offset',0),limit:Math.min(number(url,'limit',20),100)}));
      }
      if (req.method === 'GET' && url.pathname === '/team-history') return send(res,200,await required({op:'team_history'}));
      if (req.method === 'GET' && url.pathname === '/historical-identity') {
        const id=url.searchParams.get('id');
        if (!id || id.length>256) return send(res,400,{error:'Invalid historical identity'});
        return send(res,200,await required({op:'historical_identity',id}));
      }
      if (req.method === 'GET' && url.pathname === '/table') return send(res, 200, {
        status, day: snapshot.state.day, clubs: snapshot.state.clubs,
        standings: table(), dismissals: snapshot.dismissals ?? [], last_results: snapshot.results.slice(-20).map(compactMatch),
      });
      if (req.method === 'GET' && url.pathname === '/broadcast') return send(res, 200, { status, narrations });
      if (req.method === 'GET' && url.pathname === '/history') {
        const after = number(url, 'after', 0), limit = Math.min(number(url, 'limit', 100), 1000);
        if (after > snapshot.results.length) return send(res, 400, { error: 'Cursor past end' });
        return send(res, 200, { after, next: Math.min(after + limit, snapshot.results.length), total: snapshot.results.length, results: snapshot.results.slice(after, after + limit) });
      }
      if (url.pathname === '/beat' || url.pathname === '/narration') {
        if (token(req) !== credentials.narrator) return send(res, 401, { error: 'Unauthorized' });
        if (req.method === 'GET' && url.pathname === '/beat') {
          const after = number(url, 'after', 0);
          if (after > spectator.beats.length) return send(res, 400, { error: 'Cursor past end' });
          await waitFor(req, res, () => status !== 'running' || spectator.beats.length > after);
          const beats = spectator.beats.slice(after, after + 1);
          return send(res, 200, { after, next: after + beats.length, total: spectator.beats.length, cursor_kind: 'public_beats', beats,
            results: beats.flatMap(beat => beat.results), news: beats.flatMap(beat => beat.news), events: beats.flatMap(beat => beat.events),
            clubs: snapshot.state.clubs, standings: beats.at(-1)?.standings ?? [], status });
        }
        if (req.method === 'POST' && url.pathname === '/narration') {
          const item = await body(req);
          const operation = narrationChain.then(async () => {
            const last = narrations.at(-1)?.through ?? 0;
            if (!item || Object.keys(item).sort().join(',') !== 'after,text,through' ||
                !Number.isSafeInteger(item.after) || !Number.isSafeInteger(item.through) || item.after !== last ||
                item.through <= item.after || item.through > spectator.beats.length ||
                typeof item.text !== 'string' || !item.text.trim() || item.text.length > 12000) return send(res, 400, { error: 'Invalid narration coverage or text' });
            try { await writeBroadcast(broadcastFile, item); }
            catch (error) {
              console.error(JSON.stringify({ event: 'narration_failure', error: String(error?.stack ?? error).slice(0, 4096) }));
              return send(res, 503, { error: 'Narration storage unavailable' });
            }
            narrations.push(item);
            return send(res, 200, { next: item.through });
          });
          narrationChain = operation.catch(() => {});
          return await operation;
        }
      }
      if (['/observe', '/inbox', '/staff-market', '/transfer-market', '/command', '/wait'].includes(url.pathname) && !actor) return send(res, 401, { error: 'Unauthorized' });
      if (['/observe', '/inbox', '/staff-market', '/transfer-market', '/command', '/wait'].includes(url.pathname) && dismissal(actor)) {
        return send(res, req.method === 'POST' ? 403 : 200, { status: 'fired', terminal: true, dismissal: dismissal(actor) });
      }
      if (req.method === 'GET' && url.pathname === '/observe') return send(res, 200, {...await required({ op: 'observe', actor }),transfer_notice:transferNotices.get(actor)});
      if (req.method === 'GET' && url.pathname === '/staff-market') return send(res, 200, await required({op:'staff_market',actor}));
      if (req.method === 'POST' && url.pathname === '/transfer-market') return send(res,200,await required({op:'transfer_market',actor,filter:await body(req)}));
      if (req.method === 'GET' && url.pathname === '/inbox') return send(res, 200, await required({ op: 'inbox', actor, offset: number(url, 'offset', 0), limit: number(url, 'limit', 20) }));
      if (req.method === 'GET' && url.pathname === '/wait') {
        const after = number(url, 'after', snapshot.state.day);
        const afterNotice=number(url,'after_notice',transferNotices.get(actor));
        await waitFor(req, res, () => status !== 'running' || snapshot.state.day > after || transferNotices.get(actor)>afterNotice);
        if (dismissal(actor)) return send(res, 200, { status: 'fired', terminal: true, dismissal: dismissal(actor) });
        const observation = status === 'failed' ? null : {...await required({ op: 'observe', actor }),transfer_notice:transferNotices.get(actor)};
        return send(res, 200, { status, observation,transfer_notice:transferNotices.get(actor) });
      }
      if (req.method === 'POST' && url.pathname === '/command') {
        const request = await body(req);
        if (!request || Object.keys(request).sort().join(',') !== 'command,day,id') return send(res, 400, { error: 'Expected only id, day, command' });
        return await mutate(async () => {
          if (status !== 'running') return send(res, 409, { error: 'League is not running' });
          if (dismissal(actor)) return send(res, 403, { status: 'fired', terminal: true, dismissal: dismissal(actor) });
          const result = await command(actor, request);
          if (!result.ok) return send(res, 400, { error: result.error });
          if (result.data.result?.Err) return send(res, 400, { error: result.data.result.Err, receipt: result.data });
          send(res, 200, result.data);
        });
      }
      send(res, 404, { error: 'Not found' });
    } catch (error) { send(res, error.statusCode ?? 503, { error: error.statusCode ? error.message : 'Host unavailable' }); }
  });
  server.requestTimeout = 10000; server.headersTimeout = 10000; server.keepAliveTimeout = 5000;
  async function close() {
    if (closed) return;
    closed = true; clearInterval(timer); status = status === 'running' ? 'failed' : status; wake();
    pending?.reject(new Error('Host closed')); pending = undefined;
    child.kill(); server.closeAllConnections();
    await new Promise(resolveClose => server.close(resolveClose));
    await rpcChain; await mutationChain;
    // Only shutdown waits for narration, and even shutdown bounds that wait.
    let flushTimeout;
    await Promise.race([narrationChain, new Promise(resolveFlush => { flushTimeout = setTimeout(resolveFlush, 5000); })]);
    clearTimeout(flushTimeout);
    await journal.close(); await broadcastFile.close();
  }
  try {
    if (maxSeasons > 1 && !init.seasons && !init.competitions) throw new Error('Multiple seasons require career/season configuration');
    let firstDay = 7;
    if (init.seasons) {
      const start = Date.parse(`${init.career?.today}T00:00:00Z`);
      const year = Number(init.career?.today?.slice(0, 4));
      const { season_start_month: month, season_start_day: day } = init.seasons;
      let kickoff = Date.UTC(year, month - 1, day);
      if (kickoff < start) kickoff = Date.UTC(year + 1, month - 1, day);
      if (!Number.isFinite(start) || !Number.isFinite(kickoff)) throw new Error('Invalid career calendar');
      firstDay = 1 + (kickoff - start) / 86400000;
    }
    if (!init.fixtures.length && !init.competitions) init.fixtures = (await required({ op: 'schedule', club_ids: init.clubs.map(club => club.id),
      first_day: firstDay, spacing_days: init.seasons?.spacing_days ?? 7,
      seed: init.seasons?.seed ?? meta.benchmark_seed ?? init.recovery?.seed ?? 1001 })).fixtures;
    init.day = 1; init.deadline_ms = deadline; init.require_match_rosters = true;
    lastDay = init.fixtures.length ? Math.max(...init.fixtures.map(fixture => fixture.day)) : 1;
    // Full exported worlds exceed the bounded JSON-lines request size. Only the
    // trusted host supplies this local path; clients never receive filesystem ops.
    await required(init.competitions && typeof scenario === 'string'
      ? {op:'init_file',path:resolve(scenario),deadline_ms:deadline} : init);
    await refresh();
    if(init.competitions) lastDay=snapshot.season?.last_day ?? lastDay;
    const opened=Date.now();deadline=opened+dayMs;
    await required({op:'open_window',day:snapshot.state.day,now_ms:opened,deadline_ms:deadline});
    await new Promise((resolveListen, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolveListen); });
    timer = setInterval(() => {
      if (advancing || status !== 'running') return;
      advancing = true;
      advance().catch(fail).finally(() => { advancing = false; });
    }, 250);
    return { url: `http://127.0.0.1:${server.address().port}`, close };
  } catch (error) { await close(); throw error; }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const options = {};
  for (let i = 2; i < process.argv.length; i += 2) {
    const key = { '--bin': 'bin', '--scenario': 'scenario', '--out-dir': 'outDir', '--port': 'port', '--day-ms': 'dayMs', '--max-seasons': 'maxSeasons' }[process.argv[i]];
    if (!key || process.argv[i + 1] === undefined) throw new Error('Expected --bin --scenario --out-dir [--port] [--day-ms]');
    options[key] = ['port', 'dayMs', 'maxSeasons'].includes(key) ? Number(process.argv[i + 1]) : process.argv[i + 1];
  }
  const host = await startHost(options);
  console.log(host.url);
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => { host.close().then(() => process.exit(0)); });
}

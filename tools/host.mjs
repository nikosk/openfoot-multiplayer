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
  let deadline = Date.now() + dayMs;
  let lastDay = init.day;
  let ready = new Set();
  let botDay;
  let botOffersDirty = false;
  let lastOfferedSequence = 0;
  let advancing = false;
  const narrations = [];
  const waiters = new Set();
  const wake = () => { for (const callback of [...waiters]) callback(); };
  function fail(error) {
    if (closed || status === 'failed') return;
    status = 'failed'; failure = 'Host execution failed'; clearInterval(timer);
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
    return output.data;
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
    const champions = status === 'completed' && leader ? standings.filter(row => row.points === leader.points &&
      row.goals_for - row.goals_against === leader.goals_for - leader.goals_against && row.goals_for === leader.goals_for).map(row => row.club_id) : [];
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
  async function command(actor, request) {
    const result = await rpc({ op: 'command', actor, request, now_ms: Date.now() });
    if (result.ok && result.data.result?.Ok === 'Ready') ready.add(actor);
    if (result.ok && result.data.result?.Ok?.Offered && result.data.sequence > lastOfferedSequence) {
      lastOfferedSequence = result.data.sequence;
      botOffersDirty = true;
    }
    await refresh();
    return result;
  }
  async function runBots() {
    const day = snapshot.state.day;
    if (botDay === day) return;
    botDay = day;
    for (const actor of bots) {
      const own = await required({ op: 'observe', actor });
      let serial = 0;
      const issue = action => command(actor, { id: `bot-${day}-${++serial}`, day, command: action });
      // Explicit limited policy, through the same review/consent/wage rules as
      // external managers. Never silently extend expiring contracts at rollover.
      for (const [player, terms] of Object.entries(own.career?.renewal_terms ?? {})) {
        const contract = own.career.contracts[player];
        if (terms.days_remaining > 180 || contract.let_expire) continue;
        const reviewed = await issue({ Career: { Review: { action: { Renew: {
          player_id: player, weekly_wage: terms.expected_wage, years: terms.expected_years,
        } } } } });
        const preview = reviewed.data?.result?.Ok?.Career?.Preview;
        if (preview) await issue({ Career: { Confirm: { preview_id: preview.id } } });
      }
      const ranked = [...own.squad].sort((a, b) => b.ovr * b.condition - a.ovr * a.condition || a.id.localeCompare(b.id));
      const selected = [];
      for (const [position, count] of [['Goalkeeper', 1], ['Defender', 4], ['Midfielder', 4], ['Forward', 2]]) {
        selected.push(...ranked.filter(player => player.position === position).slice(0, count));
      }
      for (const player of ranked) if (selected.length < 11 && !selected.includes(player)) selected.push(player);
      if (selected.length === 11) await issue({ SetLineup: { player_ids: selected.map(player => player.id) } });
      if (own.recovery_view) await issue({ SetRecovery: { mode: 'Recovery' } });
      for (const offer of own.manager_view.offers) if (offer.status === 'Pending' && offer.seller === own.manager_view.club.id) await issue({ Reject: { offer_id: offer.id } });
      await issue('Ready');
    }
    botOffersDirty = false;
  }
  async function advance() {
    if (status !== 'running') return;
    await runBots();
    if (botOffersDirty) {
      botOffersDirty = false;
      for (const actor of bots) {
        const own = await required({ op: 'observe', actor });
        for (const offer of own.manager_view.offers) if (offer.status === 'Pending' && offer.seller === own.manager_view.club.id) {
          await command(actor, { id: `bot-reject-${offer.id}`, day: snapshot.state.day, command: { Reject: { offer_id: offer.id } } });
        }
      }
    }
    if (Date.now() < deadline && ready.size !== managers.length) return;
    const day = snapshot.state.day;
    const now = Date.now();
    deadline = now + dayMs;
    await required({ op: 'tick', day, now_ms: now, next_deadline_ms: deadline });
    ready = new Set();
    await refresh();
    if (snapshot.season_history?.length >= maxSeasons || (!init.seasons && day >= lastDay)) {
      await required({ op: 'save_file', path: resolve(outDir, 'final.checkpoint.json') });
      status = 'completed'; clearInterval(timer);
    }
    wake();
    if (status === 'running') await runBots();
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
          if (after > snapshot.results.length) return send(res, 400, { error: 'Cursor past end' });
          await waitFor(req, res, () => status !== 'running' || snapshot.results.length > after);
          return send(res, 200, { after, next: snapshot.results.length, total: snapshot.results.length, results: snapshot.results.slice(after).map(compactMatch),
            clubs: snapshot.state.clubs, standings: table(), dismissals: snapshot.dismissals ?? [], status });
        }
        if (req.method === 'POST' && url.pathname === '/narration') {
          const item = await body(req);
          const operation = narrationChain.then(async () => {
            const last = narrations.at(-1)?.through ?? 0;
            if (!item || Object.keys(item).sort().join(',') !== 'after,text,through' ||
                !Number.isSafeInteger(item.after) || !Number.isSafeInteger(item.through) || item.after !== last ||
                item.through <= item.after || item.through > snapshot.results.length ||
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
      if (['/observe', '/command', '/wait'].includes(url.pathname) && !actor) return send(res, 401, { error: 'Unauthorized' });
      if (['/observe', '/command', '/wait'].includes(url.pathname) && dismissal(actor)) {
        return send(res, req.method === 'POST' ? 403 : 200, { status: 'fired', terminal: true, dismissal: dismissal(actor) });
      }
      if (req.method === 'GET' && url.pathname === '/observe') return send(res, 200, await required({ op: 'observe', actor }));
      if (req.method === 'GET' && url.pathname === '/wait') {
        const after = number(url, 'after', snapshot.state.day);
        await waitFor(req, res, () => status !== 'running' || snapshot.state.day > after);
        if (dismissal(actor)) return send(res, 200, { status: 'fired', terminal: true, dismissal: dismissal(actor) });
        const observation = status === 'failed' ? null : await required({ op: 'observe', actor });
        return send(res, 200, { status, observation });
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
    if (maxSeasons > 1 && !init.seasons) throw new Error('Multiple seasons require career/season configuration');
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
    if (!init.fixtures.length) init.fixtures = (await required({ op: 'schedule', club_ids: init.clubs.map(club => club.id),
      first_day: firstDay, spacing_days: init.seasons?.spacing_days ?? 7,
      seed: init.seasons?.seed ?? meta.benchmark_seed ?? init.recovery?.seed ?? 1001 })).fixtures;
    init.day = 1; init.deadline_ms = deadline; init.require_match_rosters = true;
    lastDay = Math.max(...init.fixtures.map(fixture => fixture.day));
    await required(init); await refresh(); await runBots();
    await new Promise((resolveListen, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolveListen); });
    timer = setInterval(() => {
      if (advancing || status !== 'running') return;
      advancing = true;
      mutate(advance).catch(fail).finally(() => { advancing = false; });
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

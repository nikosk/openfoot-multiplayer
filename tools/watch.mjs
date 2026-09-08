#!/usr/bin/env node
// Minimal terminal consumer of the public API; no private tokens or model tools.
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:4319');
let lastNarration = 0;
let lastResults = -1;
const clean = value => String(value).replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
async function get(path) {
  const response = await fetch(new URL(path, base), { signal: AbortSignal.timeout(5000) });
  if (!response.ok) throw new Error(`Spectator HTTP ${response.status}`);
  return response.json();
}
for (;;) {
  try {
    const snapshot = await get('/public');
    if (snapshot.results.length !== lastResults) {
      const names = new Map(snapshot.state.clubs.map(club => [club.id, club.name]));
      console.log(`\nDay ${snapshot.state.day} | ${clean(snapshot.status)} | ${snapshot.results.length} matches completed`);
      console.table(snapshot.standings.map((row, i) => ({ position: i + 1, club: clean(names.get(row.club_id) ?? row.club_id), played: row.played, points: row.points, GD: row.goals_for - row.goals_against })));
      lastResults = snapshot.results.length;
    }
    const broadcast = await get('/broadcast');
    const items = Array.isArray(broadcast) ? broadcast : broadcast.narrations;
    if (!Array.isArray(items)) throw new Error('Invalid public broadcast response');
    for (const item of items.slice(lastNarration)) console.log(`\n${clean(item.text)}\n`);
    lastNarration = items.length;
  } catch (error) { console.error(clean(error.message)); }
  await new Promise(resolve => setTimeout(resolve, 2000));
}

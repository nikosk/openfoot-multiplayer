import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { replayJournal } from './replay.mjs';

async function replies(binary, inputs) {
  const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'ignore'] });
  const lines = createInterface({ input: child.stdout });
  const result = new Promise((resolveReplies, reject) => {
    const output = [];
    child.on('error', reject);
    child.stdin.on('error', reject);
    lines.on('line', line => {
      try {
        output.push(JSON.parse(line));
        if (output.length === inputs.length) resolveReplies(output);
      } catch (error) { reject(error); }
    });
    child.on('exit', () => { if (output.length !== inputs.length) reject(new Error('Missing replies')); });
  });
  const timer = setTimeout(() => child.kill(), 5000);
  try {
    child.stdin.write(inputs.map(x => JSON.stringify(x)).join('\n') + '\n');
    return await result;
  } finally { clearTimeout(timer); lines.close(); child.stdin.destroy(); child.kill(); }
}

test('private journal reproduces readiness, idempotent receipts and the advanced day', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'football-replay-test-'));
  const binary = resolve('target/debug/league');
  const command = { op: 'command', actor: 'manager', now_ms: 10, request: { id: 'ready', day: 1, command: 'Ready' } };
  const inputs = [
    { op: 'init', clubs: [{ id: 'club', name: 'Club', balance: 100 }], players: [], managers: [{ id: 'manager', club_id: 'club' }], attributes: [], fixtures: [], recovery: null, day: 1, deadline_ms: 100 },
    command, command, { op: 'tick', day: 1, now_ms: 20, next_deadline_ms: 200 }, { op: 'public' },
  ];
  try {
    const outputs = await replies(binary, inputs);
    assert.ok(outputs.every(x => x.ok));
    const entries = inputs.map((input, i) => ({ input, output: outputs[i] }));
    const journal = join(dir, 'journal.jsonl');
    await writeFile(journal, entries.map(x => JSON.stringify(x)).join('\n') + '\n', { mode: 0o600 });
    assert.deepEqual(await replayJournal(binary, journal), { verified_entries: 5 });
    entries[2].output = { ok: false, error: 'corrupted receipt' };
    await writeFile(journal, entries.map(x => JSON.stringify(x)).join('\n') + '\n');
    await assert.rejects(replayJournal(binary, journal), /mismatch at entry 3/);
    await writeFile(journal, '{"input":');
    await assert.rejects(replayJournal(binary, journal), SyntaxError);
  } finally { await rm(dir, { recursive: true }); }
});

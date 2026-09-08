#!/usr/bin/env node
// Reconstruct the local game from the private host journal, verifying each reply.
// This checks football replay, not model/session or wall-clock scheduling replay.
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';
import { isDeepStrictEqual } from 'node:util';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

export async function replayJournal(binary, journal) {
  const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  let spawnError;
  child.on('error', error => { spawnError = error; });
  child.stdin.on('error', error => { spawnError = error; });
  let stderr = '';
  child.stderr.on('data', bytes => { stderr = (stderr + bytes).slice(-16000); });
  const replies = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const records = createInterface({ input: createReadStream(journal), crlfDelay: Infinity });
  let count = 0;
  try {
    for await (const line of records) {
      const entry = JSON.parse(line);
      if (!entry.input || !entry.output) throw new Error(`Invalid journal entry ${count + 1}`);
      if (spawnError) throw spawnError;
      const reply = replies.next();
      await new Promise((res, rej) => child.stdin.write(JSON.stringify(entry.input) + '\n', error => error ? rej(error) : res()));
      let timer;
      const response = await Promise.race([reply, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Replay reply timeout')), 30000); })]).finally(() => clearTimeout(timer));
      if (response.done) throw new Error(`Game exited during replay: ${stderr}`);
      if (!isDeepStrictEqual(JSON.parse(response.value), entry.output)) throw new Error(`Replay mismatch at entry ${count + 1}`);
      count++;
    }
    if (!count) throw new Error('Empty journal');
    return { verified_entries: count };
  } finally {
    records.close();
    child.stdin.destroy();
    child.kill();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [binary, journal] = process.argv.slice(2);
  if (!binary || !journal) { console.error('Usage: node tools/replay.mjs <league-binary> <private-journal.jsonl>'); process.exitCode = 1; }
  else replayJournal(binary, journal).then(result => console.log(JSON.stringify(result))).catch(error => { console.error(error.message); process.exitCode = 1; });
}

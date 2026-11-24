import { test } from 'node:test';
import assert from 'node:assert/strict';
import { api, ApiError, setFetch } from './api';

type Call = { url: string; init?: RequestInit };

function mock(status: number, body: unknown, calls: Call[] = []) {
  setFetch((async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return new Response(typeof body === 'string' ? body : JSON.stringify(body), { status, statusText: 'X' });
  }) as typeof fetch);
  return calls;
}

test('the server message is shown verbatim on errors', async () => {
  mock(503, { code: 'steer_unsupported', message: 'This Zeroshot cannot deliver messages to running agents.' });
  await assert.rejects(api.steer('local', 'r', 1, 'hi'), (e: unknown) => {
    assert.ok(e instanceof ApiError);
    assert.equal(e.code, 'steer_unsupported');
    assert.equal(e.message, 'This Zeroshot cannot deliver messages to running agents.');
    return true;
  });
});

test('non-JSON failures still say what failed', async () => {
  mock(502, '<html>bad gateway</html>');
  await assert.rejects(api.overview(), /Could not list runs \(502\)\./);
});

test('network failures explain the server may be down, in plain words', async () => {
  setFetch((async () => {
    throw new TypeError('Failed to fetch');
  }) as typeof fetch);
  await assert.rejects(api.overview(), (e: unknown) => {
    assert.ok(e instanceof ApiError);
    assert.equal(e.message, 'Could not list runs: the zerochat server did not answer.');
    return true;
  });
});

test('steer posts JSON to the run, target ids are encoded', async () => {
  const calls = mock(200, { status: 'delivered' });
  const result = await api.steer('target:lab', 'run-1', 7, 'Check the logs');
  assert.deepEqual(result, { status: 'delivered' });
  assert.equal(calls[0].url, '/api/runs/target%3Alab/run-1/steer');
  assert.equal(calls[0].init?.method, 'POST');
  assert.equal((calls[0].init!.headers as Record<string, string>)['content-type'], 'application/json');
  assert.deepEqual(JSON.parse(String(calls[0].init?.body)), { execution: 7, message: 'Check the logs' });
});

test('replay sends only what was given; ranges become query strings', async () => {
  const calls = mock(200, { valid: true });
  await api.replay('local', 'r', { checkpointId: 'c1', validateOnly: true });
  assert.deepEqual(JSON.parse(String(calls[0].init?.body)), { checkpointId: 'c1', validateOnly: true });
  mock(200, { files: [] }, calls);
  await api.changes('local', 'r', { from: 'abc', to: null });
  assert.equal(calls[1].url, '/api/runs/local/r/changes?from=abc');
  mock(200, { diff: '' }, calls);
  await api.diff('local', 'r', { path: 'src/a b.ts' });
  assert.equal(calls[2].url, '/api/runs/local/r/diff?path=src%2Fa+b.ts');
});

test('resume without a checkpoint sends an empty body', async () => {
  const calls = mock(200, {});
  await api.resume('local', 'r');
  assert.equal(String(calls[0].init?.body), '{}');
});

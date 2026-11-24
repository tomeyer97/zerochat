// Tool calls whose input Zeroshot cut in its log: the server's transcript of them (the Rust
// golden `transcript/ledger/cut-tool-calls.json`) as the chat's tool cards show them.
import { API_DIR, checkGolden } from './testdata/golden';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { INPUT_CUT_NOTE, toolCardView, toolSummary } from './transcript-model';
import type { Transcript } from './generated/transcript';

const transcript = JSON.parse(fs.readFileSync(path.resolve(API_DIR, '../transcript/ledger/cut-tool-calls.json'), 'utf8')) as Transcript;
const cards = transcript.items.map((item) => toolCardView(item, item.tool!, false, '/w'));

test('golden: a cut call reads as a tool line with its result, an uncut one as before', () => {
  checkGolden('transcript-model/toolCardView-cut', cards);
});

test('a cut Bash call shows its first line, … and the cut note, in neutral style', () => {
  const [bash, edit, read, uncut] = cards;
  assert.deepEqual([bash.verb, bash.detail, bash.cut, bash.status], ['Bash', "cd /w && python3 - <<'EOF' …", INPUT_CUT_NOTE, 'ok']);
  assert.equal(`${bash.verb} · ${bash.detail} ${bash.cut}`, "Bash · cd /w && python3 - <<'EOF' … (input cut by Zeroshot)");
  assert.deepEqual([edit.verb, edit.detail, edit.cut, edit.status], ['Edit', 'src/a.ts …', INPUT_CUT_NOTE, 'error']);
  assert.deepEqual([read.verb, read.detail, read.status], ['Read', 'README.md …', 'ok']);
  assert.deepEqual([uncut.verb, uncut.detail, uncut.cut], ['Bash', 'ls', null]);
  // Not raw JSON any more.
  assert.ok(!bash.detail.startsWith('{'));
  // Nothing readable left: the tool's name and the note.
  const bare = toolCardView({ kind: 'tool', at: null }, { id: 'x', name: 'Bash', input: {}, inputCut: true }, false, null);
  assert.deepEqual([bare.verb, bare.detail, bare.cut], ['Bash', '', INPUT_CUT_NOTE]);
  assert.deepEqual(toolSummary({ name: 'Read', input: { file_path: '/w/a' } }), { verb: 'Read', detail: '/w/a' });
});

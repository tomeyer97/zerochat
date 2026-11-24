// The web's copies of display rules the Rust side also applies, checked against what the Rust side
// does (web/src/generated/ is written from it, `make types`).
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { shortModel } from './format';
import { SHORT_MODEL_EXAMPLES } from './generated/projection';

test('shortModel names a model the way projection.rs short_model does', () => {
  assert.ok(SHORT_MODEL_EXAMPLES.length > 0);
  for (const [id, short] of SHORT_MODEL_EXAMPLES) assert.equal(shortModel(id), short, id);
});

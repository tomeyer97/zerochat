import { test } from 'node:test';
import assert from 'node:assert/strict';
import { compact, costText, duration, fullTime, offsetText, relativeAge, shortModel, timeOfDay, tokenParts, totalTokens, usageBreakdown, usd } from './format';

test('durations read like a person would say them', () => {
  assert.equal(duration(null), '—');
  assert.equal(duration(450), '450ms');
  assert.equal(duration(42_000), '42s');
  assert.equal(duration(185_000), '3m 05s');
  assert.equal(duration(2 * 3600_000 + 14 * 60_000), '2h 14m');
  assert.equal(duration(80 * 3600_000), '3d 8h');
  assert.equal(duration(-5), '0ms');
});

test('relative ages', () => {
  const now = 1_000_000_000_000;
  assert.equal(relativeAge(now - 10_000, now), 'just now');
  assert.equal(relativeAge(now - 5 * 60_000, now), '5m ago');
  assert.equal(relativeAge(now - 3 * 3600_000, now), '3h ago');
  assert.equal(relativeAge(now - 3 * 86_400_000, now), '3d ago');
  assert.equal(relativeAge(null, now), '');
});

test('money and tokens', () => {
  assert.equal(usd(null), '—');
  assert.equal(usd(0.004), '<$0.01');
  assert.equal(usd(17.4012), '$17.40');
  assert.equal(usd(0), '$0.00');
  assert.equal(compact(950), '950');
  assert.equal(compact(1234), '1.2k');
  assert.equal(compact(43_716_938), '44M');
  assert.equal(compact(2_500_000), '2.5M');
  const usage = { inputTokens: 1, outputTokens: 2, cacheReadInputTokens: 3, cacheCreationInputTokens: 4 };
  assert.equal(totalTokens(usage), 10);
  assert.match(usageBreakdown(usage), /Cache write: 4/);
});

test('model names lose the vendor prefix and date', () => {
  assert.equal(shortModel('claude-haiku-4-5-20251001'), 'haiku-4-5');
  assert.equal(shortModel('claude-sonnet-5'), 'sonnet-5');
  assert.equal(shortModel('gpt-5-codex'), 'gpt-5-codex');
});

test('one date format: short in lines, exact in tooltips, the year only when it differs', () => {
  const now = new Date(2026, 10, 4, 15, 0).getTime();
  const today = new Date(2026, 10, 4, 14, 21, 5).getTime();
  const lastWeek = new Date(2026, 9, 28, 14, 21).getTime();
  const lastYear = new Date(2025, 9, 28, 14, 21).getTime();
  assert.doesNotMatch(timeOfDay(today, now), /Nov|2026/);
  assert.match(timeOfDay(lastWeek, now), /Oct 28/);
  assert.doesNotMatch(timeOfDay(lastWeek, now), /2026/);
  assert.match(timeOfDay(lastYear, now), /2025/);
  assert.notEqual(timeOfDay(lastWeek, now), timeOfDay(lastYear, now));
  assert.match(fullTime(today, now), /Nov 4.*2:21:05/);
  assert.match(fullTime(lastYear, now), /2025/);
  assert.equal(offsetText(today, today - 65_400), '+1m 05s');
});

test('cost and cache splits that say nothing are left out', () => {
  assert.equal(costText(0, true), '—');
  assert.equal(costText(0.23, true), '~$0.23');
  assert.equal(costText(0.23, false), '$0.23');
  // Exactly all cached: the split is dropped because it is literally the same number.
  const allCached = { inputTokens: 0, outputTokens: 0, cacheReadInputTokens: 1_500_000, cacheCreationInputTokens: 0 };
  assert.equal(tokenParts(allCached).cached, null);
  // Only near-equal once rounded to the same compact string (1.5M vs 1.5M): still shown.
  const nearlyAllCached = { inputTokens: 10, outputTokens: 10, cacheReadInputTokens: 1_500_000, cacheCreationInputTokens: 0 };
  assert.equal(tokenParts(nearlyAllCached).cached, '1.5M cached');
  const mostly = { inputTokens: 100_000, outputTokens: 10, cacheReadInputTokens: 800_000, cacheCreationInputTokens: 0 };
  assert.equal(tokenParts(mostly).cached, '800k cached');
});

const assert = require('node:assert/strict');
const test = require('node:test');
const { proposeCleanUrl } = require('../frontend/src/url-cleaner.ts');

test('removes selected trackers while preserving exact URI spelling', () => {
  const original = 'https://EXAMPLE.test/a%2fb?sort=a%2Fb&u%74m_source=a+b&keep=a+b&fbclid=b&keep=%2f#section?stay=1';
  assert.deepEqual(proposeCleanUrl(original), {
    status: 'changes',
    cleaned: 'https://EXAMPLE.test/a%2fb?sort=a%2Fb&keep=a+b&keep=%2f#section?stay=1',
    removed: ['utm_source', 'fbclid'],
  });
});

test('leaves signed and authenticated addresses alone', () => {
  for (const key of ['X-Amz-Signature', 'X-Goog-Signature', 'signature', '%73ig', 'token', 'key-pair-id', 'auth']) {
    const url = 'https://example.test/report?utm_source=test&' + key + '=sample';
    assert.deepEqual(proposeCleanUrl(url), { status: 'protected' }, key);
  }
});

test('retains business and functional parameters unless explicitly selected', () => {
  const url = 'https://example.test/report?affiliate=museum&campaign=spring&sort=descending&gclid=test';
  assert.equal(proposeCleanUrl(url).cleaned,
    'https://example.test/report?affiliate=museum&campaign=spring&sort=descending');
  assert.equal(proposeCleanUrl(url, new Set(['gclid', 'campaign'])).cleaned,
    'https://example.test/report?affiliate=museum&sort=descending');
});

test('keeps URL fragments and empty non-tracking segments', () => {
  assert.equal(proposeCleanUrl('https://example.test/?utm_source=1#frag').cleaned, 'https://example.test/#frag');
  assert.equal(proposeCleanUrl('https://example.test/?utm_source=1&&a=1#').cleaned, 'https://example.test/?&a=1#');
  assert.deepEqual(proposeCleanUrl('https://example.test/#?utm_source=1'), { status: 'unchanged' });
});

test('declines invalid URLs and respects hostname exceptions', () => {
  for (const url of ['file:///etc/hosts', 'https://untrusted:pass@example.test?a=%xx', 'https://example.test/?utm_source=%f', 'https://example.test/ https://elsewhere.test/?utm_source=a']) {
    assert.deepEqual(proposeCleanUrl(url), { status: 'invalid' });
  }
  const url = 'https://downloads.example.test/a?utm_source=test';
  assert.deepEqual(proposeCleanUrl(url, undefined, new Set(['downloads.example.test'])), { status: 'protected' });
});

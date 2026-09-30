// index.html's link-preview tags: present, absolute (the servers that build
// previews fetch the page from outside), and pointing at a picture that is
// actually in the repo at the size the tags claim.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const html = fs.readFileSync(path.join(ROOT, 'index.html'), 'utf8');
const meta = (attr, key) => html.match(new RegExp(`<meta ${attr}="${key}" content="([^"]*)"`))?.[1];

test('link previews: title, description, url and a large image', () => {
  assert.equal(meta('property', 'og:title'), 'Midnight Racer');
  assert.ok(meta('property', 'og:description')?.length > 40);
  assert.equal(meta('name', 'description'), meta('property', 'og:description'));
  assert.equal(meta('name', 'twitter:card'), 'summary_large_image');
  const url = meta('property', 'og:url'), image = meta('property', 'og:image');
  assert.match(url, /^https:\/\/.+\/$/, 'og:url is absolute');
  assert.ok(image?.startsWith(url), 'og:image is absolute, on the same site');
});

test('link previews: the image is in the repo, a 1200 × 630 PNG', () => {
  const url = meta('property', 'og:url'), image = meta('property', 'og:image');
  const file = path.join(ROOT, image.slice(url.length));
  const png = fs.readFileSync(file);
  assert.equal(png.subarray(1, 4).toString(), 'PNG');
  assert.equal(png.readUInt32BE(16), Number(meta('property', 'og:image:width')));
  assert.equal(png.readUInt32BE(20), Number(meta('property', 'og:image:height')));
  assert.deepEqual([png.readUInt32BE(16), png.readUInt32BE(20)], [1200, 630]);
});

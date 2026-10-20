// The retired separate Community build and the URL-fragment transfer are gone for good.
import { readdirSync, readFileSync } from 'node:fs';
import { expect, it } from 'vitest';

it('no source mentions VITE_COMMUNITY, LocalCommunity, blockTransfer, workspaceLink or #import-block', () => {
  const files = (readdirSync('src', { recursive: true }) as string[]).filter((f) => /\.(ts|tsx)$/.test(f) && !f.endsWith('cleanup.test.ts'));
  const hits = files.filter((f) => /VITE_COMMUNITY|LocalCommunity|blockTransfer|workspaceLink|#import-block/.test(readFileSync(`src/${f}`, 'utf8')));
  expect(hits).toEqual([]);
});

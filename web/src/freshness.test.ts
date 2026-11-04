import { describe, expect, it } from 'vitest';
import { bundleIn } from './freshness';

describe('bundleIn', () => {
  it('finds the entry bundle in a built index.html', () => {
    const html = '<script type="module" crossorigin src="./assets/index-BDSpvP5x.js"></script>';
    expect(bundleIn(html)).toBe('index-BDSpvP5x.js');
    expect(bundleIn('<script src="/src/main.tsx"></script>')).toBeNull();
  });
});

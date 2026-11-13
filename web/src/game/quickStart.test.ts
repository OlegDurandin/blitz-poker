import { expect, it } from 'vitest';
import { quickStartTarget } from './quickStart';
it('prefers authoritative Solo for authenticated online starts and resumes an existing network game', () => {
  expect(quickStartTarget(true, true, true, null)).toBe('server');
  expect(quickStartTarget(true, true, true, 'already-playing')).toBe('resume');
});
it.each([[false,true,true],[true,false,true],[true,true,false]])('keeps local fallback for configuration=%s account=%s network=%s', (configured, account, online) => {
  expect(quickStartTarget(configured, account, online, null)).toBe('local');
});

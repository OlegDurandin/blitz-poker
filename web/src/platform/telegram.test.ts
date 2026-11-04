import { describe, expect, it } from 'vitest';
import { roomFromStartParam } from './telegram';

const ID = 'O6ceAbCdEfGhIjKlMn0_-q';

describe('roomFromStartParam', () => {
  it('reads an invite from startapp', () => {
    expect(roomFromStartParam(`room_${ID}`)).toBe(ID);
  });
  it('ignores other start parameters', () => {
    expect(roomFromStartParam(null)).toBeNull();
    expect(roomFromStartParam('promo')).toBeNull();
    expect(roomFromStartParam('room_short')).toBeNull();
    expect(roomFromStartParam(`room_${ID}!`)).toBeNull();
  });
});

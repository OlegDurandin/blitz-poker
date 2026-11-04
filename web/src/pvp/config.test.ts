import { describe, expect, it } from 'vitest';
import { parseRoom } from './config';

const ID = 'O6ceAbCdEfGhIjKlMn0_-q';

describe('parseRoom', () => {
  it('reads the fragment in the forms VK clients hand over', () => {
    expect(parseRoom(`#room=${ID}`)).toBe(ID);
    expect(parseRoom(`#/room=${ID}`)).toBe(ID);
    expect(parseRoom(`#room%3D${ID}`)).toBe(ID);
    expect(parseRoom(`room%253D${ID}`)).toBe(ID);
  });

  it('reads a pasted link or a bare id', () => {
    expect(parseRoom(`  https://vk.com/app123#room=${ID}\n`)).toBe(ID);
    expect(parseRoom(`Заходи! https://vk.com/app123#room=${ID} жду`)).toBe(ID);
    expect(parseRoom(ID)).toBe(ID);
  });

  it('rejects anything else', () => {
    expect(parseRoom('')).toBeNull();
    expect(parseRoom(null)).toBeNull();
    expect(parseRoom('https://vk.com/app123')).toBeNull();
    expect(parseRoom('#room=short')).toBeNull();
    expect(parseRoom('%E0%A4%A')).toBeNull();
  });
});

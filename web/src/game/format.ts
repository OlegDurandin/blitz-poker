import type { ActionRecord, Card, Category, ShownHand, Street } from '../engine/types';

const RANK_LABEL: Record<string, string> = { T: '10' };
const RANK_NAME: Record<string, string> = {
  A: 'Туз',
  K: 'Король',
  Q: 'Дама',
  J: 'Валет',
  T: 'Десятка',
  '9': 'Девятка',
  '8': 'Восьмёрка',
  '7': 'Семёрка',
  '6': 'Шестёрка',
  '5': 'Пятёрка',
  '4': 'Четвёрка',
  '3': 'Тройка',
  '2': 'Двойка',
};
const SUIT_NAME: Record<string, string> = { s: 'пик', h: 'червей', d: 'бубен', c: 'треф' };
export const SUIT_SYMBOL: Record<string, string> = { s: '♠', h: '♥', d: '♦', c: '♣' };

export function rankLabel(card: Card): string {
  return RANK_LABEL[card[0]] ?? card[0];
}

export function suitOf(card: Card): string {
  return card[1];
}

export function isRed(card: Card): boolean {
  return card[1] === 'h' || card[1] === 'd';
}

/** Accessible name, e.g. "Туз пик". */
export function cardName(card: Card): string {
  return `${RANK_NAME[card[0]]} ${SUIT_NAME[card[1]]}`;
}

export const CATEGORY_NAME: Record<Category, string> = {
  high_card: 'Старшая карта',
  pair: 'Пара',
  two_pair: 'Две пары',
  three_of_a_kind: 'Тройка',
  straight: 'Стрит',
  flush: 'Флеш',
  full_house: 'Фулл-хаус',
  four_of_a_kind: 'Каре',
  straight_flush: 'Стрит-флеш',
  royal_flush: 'Роял-флеш',
};

// Rank word forms, indexed by the rank character.
const RANKS = '23456789TJQKA';
const NOM = ['двойка', 'тройка', 'четвёрка', 'пятёрка', 'шестёрка', 'семёрка', 'восьмёрка', 'девятка', 'десятка', 'валет', 'дама', 'король', 'туз'];
/** "до девятки" */
const GEN = ['двойки', 'тройки', 'четвёрки', 'пятёрки', 'шестёрки', 'семёрки', 'восьмёрки', 'девятки', 'десятки', 'валета', 'дамы', 'короля', 'туза'];
/** "валеты и короли" */
const NOM_PL = ['двойки', 'тройки', 'четвёрки', 'пятёрки', 'шестёрки', 'семёрки', 'восьмёрки', 'девятки', 'десятки', 'валеты', 'дамы', 'короли', 'тузы'];
/** "пара валетов" */
const GEN_PL = ['двоек', 'троек', 'четвёрок', 'пятёрок', 'шестёрок', 'семёрок', 'восьмёрок', 'девяток', 'десяток', 'валетов', 'дам', 'королей', 'тузов'];

const rankOf = (c: Card) => RANKS.indexOf(c[0]);

/**
 * Names the made hand with its ranks, e.g. "пара валетов", "фулл-хаус:
 * валеты и короли". `best` comes from the engine, most significant card first.
 */
export function describeHand(h: Pick<ShownHand, 'category' | 'best'>): string {
  const r = h.best.map(rankOf);
  switch (h.category) {
    case 'high_card':
      return `старшая карта: ${NOM[r[0]]}`;
    case 'pair':
      return `пара ${GEN_PL[r[0]]}`;
    case 'two_pair':
      return `две пары: ${NOM_PL[r[0]]} и ${NOM_PL[r[2]]}`;
    case 'three_of_a_kind':
      return `тройка: ${NOM_PL[r[0]]}`;
    case 'straight':
      return `стрит до ${GEN[r[0]]}`;
    case 'flush':
      return `флеш до ${GEN[r[0]]}`;
    case 'full_house':
      return `фулл-хаус: ${NOM_PL[r[0]]} и ${NOM_PL[r[3]]}`;
    case 'four_of_a_kind':
      return `каре: ${NOM_PL[r[0]]}`;
    case 'straight_flush':
      return `стрит-флеш до ${GEN[r[0]]}`;
    case 'royal_flush':
      return 'роял-флеш';
  }
}

/** Cards that form the combination itself; the rest are kickers. */
const MADE_CARDS: Partial<Record<Category, number>> = {
  high_card: 1,
  pair: 2,
  two_pair: 4,
  three_of_a_kind: 3,
  four_of_a_kind: 4,
};

/**
 * When both players made the same combination and a kicker decided the pot,
 * says which, e.g. "Решил кикер: король против девятки". Display only: the
 * engine has already chosen the winner.
 */
export function kickerNote(mine: ShownHand, theirs: ShownHand): string | null {
  const made = MADE_CARDS[mine.category];
  if (made === undefined || mine.category !== theirs.category) return null;
  for (let i = 0; i < 5; i++) {
    const a = rankOf(mine.best[i]);
    const b = rankOf(theirs.best[i]);
    if (a === b) continue;
    if (i < made) return null; // the combinations themselves differ; the names say so
    const [hi, lo] = a > b ? [a, b] : [b, a];
    return `Решил кикер: ${NOM[hi]} против ${GEN[lo]}`;
  }
  return null;
}


export const STREET_NAME: Record<Street, string> = {
  preflop: 'Префлоп',
  flop: 'Флоп',
  turn: 'Тёрн',
  river: 'Ривер',
};


/** Short table label for a recorded action. */
export function actionLabel(a: ActionRecord): string {
  if (a.all_in && (a.kind === 'call' || a.kind === 'bet' || a.kind === 'raise')) {
    return `Олл-ин ${a.to}`;
  }
  switch (a.kind) {
    case 'small_blind':
      return `МБ ${a.amount}`;
    case 'big_blind':
      return `ББ ${a.amount}`;
    case 'fold':
      return 'Сброс';
    case 'check':
      return 'Чек';
    case 'call':
      return `Колл ${a.amount}`;
    case 'bet':
      return `Ставка ${a.to}`;
    case 'raise':
      return `Рейз до ${a.to}`;
    case 'return':
      return `Возврат ${a.amount}`;
  }
}

export function signed(n: number): string {
  return n > 0 ? `+${n}` : n < 0 ? `−${-n}` : '0';
}

/** "1 раздача", "3 раздачи", "8 раздач". */
export function plural(n: number, one: string, few: string, many: string): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return one;
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return few;
  return many;
}

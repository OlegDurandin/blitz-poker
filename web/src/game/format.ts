import type { ActionRecord, Card, Category, Street } from '../engine/types';

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

/** Lowercase form for sentences: "Вы — две пары". */
export function categoryLower(c: Category): string {
  return CATEGORY_NAME[c].toLowerCase();
}

export const STREET_NAME: Record<Street, string> = {
  preflop: 'Префлоп',
  flop: 'Флоп',
  turn: 'Тёрн',
  river: 'Ривер',
};

export const BOT_NAME = 'Бот Алекс';

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

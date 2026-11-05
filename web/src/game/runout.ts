import { useEffect, useState } from 'react';
import { prefersReducedMotion } from './motion';

/**
 * Board cards revealed so far during an all-in runout. The engine resolves the
 * hand at once; this only paces the reveal and can be skipped.
 * `key` names the hand; `from` is the board length when the runout began.
 */
export function useRunoutReveal(key: string, from: number | null, boardLength: number) {
  const [progress, setProgress] = useState<{ key: string; count: number }>({ key: '', count: 5 });

  useEffect(() => {
    if (from === null) return;
    let count = from;
    setProgress({ key, count });
    const step = prefersReducedMotion() ? 350 : 750;
    const timer = window.setInterval(() => {
      count = count < 3 ? 3 : count + 1;
      setProgress({ key, count });
      if (count >= 5) window.clearInterval(timer);
    }, step);
    return () => window.clearInterval(timer);
  }, [key, from]);

  const count = from === null ? boardLength : progress.key === key ? progress.count : from;
  return {
    visible: Math.min(count, boardLength),
    revealing: from !== null && count < 5,
    skip: () => setProgress({ key, count: 5 }),
  };
}

export function streetFor(boardLen: number) {
  return boardLen >= 5 ? 'river' : boardLen === 4 ? 'turn' : boardLen === 3 ? 'flop' : 'preflop';
}

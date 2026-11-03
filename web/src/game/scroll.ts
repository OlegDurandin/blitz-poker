import { useLayoutEffect } from 'react';

// Lists restore their scroll position when the user comes back from a detail
// screen (match list → hands → replay and back).
const positions = new Map<string, number>();

export function useScrollMemory(key: string): void {
  useLayoutEffect(() => {
    window.scrollTo(0, positions.get(key) ?? 0);
    return () => {
      positions.set(key, window.scrollY);
    };
  }, [key]);
}

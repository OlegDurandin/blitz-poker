import { setTransport } from '../game/analytics';
import { accountKey, personal } from '../profile/api';
/** Reuse the product buffer and authenticated server outbox. No cards, launch
 * parameters or arbitrary event params go into this diagnostic transport. */
export function connectDailyAnalytics() {
  setTransport((event) => {
    try {
      if (!accountKey() || (event !== 'daily_challenges_viewed' && event !== 'daily_challenges_play_clicked')) return;
      // getRandomValues also works in WebViews without crypto.randomUUID.
      const eventId = Array.from(crypto.getRandomValues(new Uint8Array(16)), b => b.toString(16).padStart(2, '0')).join('');
      void personal('daily-analytics', { event, event_id: eventId }).catch(() => {});
    } catch { /* Analytics must never block opening a screen or starting a game. */ }
  });
}

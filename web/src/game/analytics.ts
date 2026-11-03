// Minimal product analytics (spec §11). No personal data is collected.
// Events are buffered; plug a real transport into `setTransport` when ready.

export type AnalyticsEvent =
  | 'app_open'
  | 'start_click'
  | 'tutorial_started'
  | 'tutorial_skipped'
  | 'tutorial_completed'
  | 'match_started'
  | 'hand_completed'
  | 'match_paused'
  | 'match_resumed'
  | 'match_completed'
  | 'match_abandoned'
  | 'rematch_clicked'
  | 'hand_history_opened'
  | 'match_history_opened'
  | 'hand_replay_opened'
  | 'hand_replay_stepped'
  | 'bot_action_selected'
  | 'persistence_error';

type Params = Record<string, string | number | boolean | null>;
type Transport = (event: AnalyticsEvent, params: Params) => void;

let transport: Transport | null = null;
const buffer: Array<[AnalyticsEvent, Params]> = [];

export function setTransport(t: Transport): void {
  transport = t;
  for (const [event, params] of buffer.splice(0)) t(event, params);
}

export function track(event: AnalyticsEvent, params: Params = {}): void {
  if (import.meta.env.DEV) console.debug('[analytics]', event, params);
  if (transport) transport(event, params);
  else if (buffer.length < 200) buffer.push([event, params]);
}

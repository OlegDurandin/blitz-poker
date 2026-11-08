import { platform } from '../platform';

export interface FeedbackSettings { sound: boolean; haptics: boolean }
const KEY = 'blitz.feedback.v1';
const defaults: FeedbackSettings = { sound: false, haptics: true };
export function feedbackSettings(): FeedbackSettings {
  try { const s = JSON.parse(localStorage.getItem(KEY) ?? '{}'); return { sound: s.sound === true, haptics: s.haptics !== false }; } catch { return { ...defaults }; }
}
export function saveFeedback(settings: FeedbackSettings) { try { localStorage.setItem(KEY, JSON.stringify(settings)); } catch { /* Private browsing. */ } }
let audio: AudioContext | undefined;
/** Called only from an explicit user action; never drives game state. */
export function actionFeedback() {
  const settings = feedbackSettings();
  if (settings.haptics) platform.haptic();
  if (!settings.sound || typeof AudioContext === 'undefined') return;
  try {
    audio ??= new AudioContext();
    void audio.resume().catch(() => {});
    const tone = audio.createOscillator();
    const gain = audio.createGain();
    tone.frequency.value = 520;
    gain.gain.setValueAtTime(0.025, audio.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.08);
    tone.connect(gain).connect(audio.destination);
    tone.start(); tone.stop(audio.currentTime + 0.08);
  } catch { /* No audio support must never block an action. */ }
}

import { useState } from 'react';
import { feedbackSettings, saveFeedback } from '../game/feedback';
export function FeedbackSettings() {
  const [settings, setSettings] = useState(feedbackSettings);
  return <fieldset className="feedback-settings"><legend>Отклик</legend>
    {(['sound', 'haptics'] as const).map(key => <label key={key}>
      <span>{key === 'sound' ? 'Звук действий' : 'Виброотклик'}</span>
      <input type="checkbox" checked={settings[key]} onChange={e => { const next = { ...settings, [key]: e.target.checked }; setSettings(next); saveFeedback(next); }} />
    </label>)}
  </fieldset>;
}

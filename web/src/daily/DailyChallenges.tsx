import { Dialog } from '../components/Dialog';
import { track } from '../game/analytics';
import { useEffect } from 'react';
import { countdown, type DailyItem, type DailyState } from './useDailyChallenges';

export function DailyCard({ state: s, onOpen }: { state: DailyState; onOpen: () => void }) {
  const item = s.data?.items.find(i => i.status === 'in_progress');
  return <button className="daily-home" onClick={onOpen}>
    <span className="daily-heading"><strong>🎯 Задачи дня</strong><span>{s.data ? `${s.data.completed_count}/${s.data.total_count}` : '›'}</span></span>
    {s.data ? <><small>Обновление через {countdown(s.remaining)}</small><span>{item ? `${item.title} · ${item.progress}/${item.target}` : `Все задачи выполнены! Бонус +${s.data.completion_bonus.awarded_xp} XP`}</span>{item && <progress value={item.progress} max={item.target} aria-label={item.title} />}</> : <small>{!s.available ? 'Откройте в VK или Telegram, чтобы сохранять прогресс' : s.error ? 'Не удалось загрузить · Повторить' : 'Загружаем задачи…'}</small>}
    <span className="daily-home__link">Все задачи ›</span>
  </button>;
}
export function DailyScreen({ state: s, onClose, onPlay }: { state: DailyState; onClose: () => void; onPlay: () => void }) {
  useEffect(() => { track('daily_challenges_viewed'); void s.refresh(); }, [s.refresh]);
  const data = s.data;
  return <Dialog title="Задачи дня" onClose={onClose}>
    <section className="daily-screen">
      <header className="profile-top"><button className="btn btn--ghost" onClick={onClose} aria-label="Назад">‹ Назад</button><h1>Задачи дня</h1></header>
      <p className="profile-note">Задачи выполняются автоматически за сетевым столом — с друзьями или ботами.</p>
      <details className="daily-help"><summary>Какие игры засчитываются?</summary><p className="profile-note">Авторизуйтесь в VK или Telegram и нажмите «Быстрая игра» — Solo с ботом запустится на сервере. Засчитываются также сетевые дуэли и столы. Локальная игра при отсутствии сети не засчитывается.</p></details>
      {!s.available && <p role="status">Откройте приложение через VK или Telegram, чтобы сохранять задачи и XP.</p>}
      {s.loading && !data && <div className="daily-skeleton" role="status">Загружаем три задачи…</div>}
      {s.error && <div role="alert"><p>{s.error}</p><button className="btn btn--secondary btn--block" onClick={() => void s.refresh()}>Повторить</button></div>}
      {data && <>
        {data.xp_budget && <p className="profile-note">XP сегодня: {data.xp_budget.awarded_today}/{data.xp_budget.total_cap}<br />Из них с ботами: {data.xp_budget.bot_awarded_today}/{data.xp_budget.bot_cap}. Доступно с ботами: {data.xp_budget.remaining_bot} XP.</p>}
        <p>Выполнено {data.completed_count} из {data.total_count}<br /><small>Обновятся через {countdown(s.remaining)} · 00:00 UTC</small></p>
        {!data.rewards_enabled && <p role="status">Начисление XP за задачи временно отключено. Прогресс сохраняется.</p>}
        {data.items.length === 0 && <p role="status">Задачи пока недоступны. Попробуйте позже.</p>}
        <div className="daily-items">{data.items.map(item => <Task key={item.id} item={item} />)}</div>
        <div className="daily-bonus"><strong>{data.completion_bonus.status === 'rewarded' ? `✓ Бонус получен: +${data.completion_bonus.awarded_xp} XP` : `Выполни все ${data.total_count} → до +${data.completion_bonus.reward_xp} XP`}</strong><span>{data.completed_count}/{data.total_count}</span>
          {data.completion_bonus.status === 'rewarded' && data.completion_bonus.awarded_xp < data.completion_bonus.reward_xp && <small>{data.completion_bonus.reward_reason === 'disabled' ? 'Начисление XP отключено' : 'Лимит XP на сегодня'}</small>}
        </div>
        <p className="profile-note">Награды начисляются автоматически в общий прогресс. Действуют общие дневные лимиты XP, включая лимит игры с ботами.</p>
      </>}
      <footer className="daily-play"><button className="btn btn--primary btn--block" onClick={() => { track('daily_challenges_play_clicked'); onPlay(); }}>Играть</button></footer>
    </section>
  </Dialog>;
}
function Task({ item: i }: { item: DailyItem }) {
  const done = i.status !== 'in_progress';
  return <article className={`daily-item${done ? ' daily-item--done' : ''}`}>
    <div className="daily-heading"><strong><span aria-hidden="true">{done ? '✓' : '○'}</span> {i.title}</strong><span>{done ? `+${i.awarded_xp} XP` : `до +${i.reward_xp} XP`}</span></div>
    <progress value={i.progress} max={i.target} aria-label={i.title} />
    <small>{i.progress}/{i.target}{done ? ' · Выполнено' : ''}{i.reward_reason === 'xp_limit' ? ' · Лимит XP на сегодня' : i.reward_reason === 'disabled' ? ' · Начисление XP отключено' : ''}</small>
  </article>;
}

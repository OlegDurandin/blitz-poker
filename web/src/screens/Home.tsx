import { useEffect, useState, type ReactNode } from 'react';
import { platform, type PlatformUser } from '../platform';
import { FeedbackSettings } from '../components/FeedbackSettings';
import { Dialog } from '../components/Dialog';
import { BotAvatar, QuickPlayIcon, SpadeLogo } from '../components/icons';
import { Difficulty } from '../components/OpponentPicker';
import type { Opponent } from '../engine/types';
import { opponentInfo } from '../game/opponents';
import { completedMatches, type Stats } from '../game/storage';
import { BUILD } from '../pvp/config';

interface Props {
  dailyCard?: ReactNode;
  stats: Stats;
  hasSavedMatch: boolean;
  tutorialInProgress: boolean;
  justFinished: boolean;
  onStart: () => void;
  onContinue: () => void;
  onRestart: () => void;
  onContinueTutorial: () => void;
  onSkipTutorial: () => void;
  onRules: () => void;
  onStats: () => void;
  onProfile?: () => void;
  opponent: Opponent;
  onPickOpponent: () => void;
  /** "Играть с другом"; absent when no duel server is configured. */
  onDuel?: () => void;
  onFriends?: () => void;
  onResumeRoom?: () => void;
  /** "Войти по коду": a friend's room by its six digits. */
  onJoinByCode?: () => void;
  /** "Третий не лишний": a table with two or three bots. */
  onTable: () => void;
  hasSavedTable: boolean;
}

export function Home(p: Props) {
  const [profile, setProfile] = useState<PlatformUser | null>(null);
  const [settings, setSettings] = useState(false);
  useEffect(() => { let stopped = false; void platform.getUser().then(user => { if (!stopped) setProfile(user); }).catch(() => {}); return () => { stopped = true; }; }, []);
  const [modes, setModes] = useState(false);
  const played = completedMatches(p.stats) + p.stats.abandoned;
  return (
    <main className="home">
      <header className="brand">
        <SpadeLogo />
        <span className="brand__name">BLITZ POKER</span>
        {p.onProfile ? <button className="home-profile" onClick={p.onProfile} aria-label="Мой профиль">{profile?.photo && <img src={profile.photo} alt="" />}<span>Профиль</span></button> : profile && <span className="home-profile">{profile.photo && <img src={profile.photo} alt="" />}<span>{profile.name}</span></span>}
      </header>

      <div className="home__intro">
        <h1 className="home__title">Твой стол ждёт!</h1>
        <p className="home__subtitle">Холдем с друзьями и ботами</p>
        {import.meta.env.DEV && <p className="home__build">Версия {BUILD}</p>}
      </div>

      <section className="home-feature" aria-label="Игра за общим столом">
        <span className="home-feature__badge">{p.onFriends ? 'VK + Telegram' : 'Вы и боты'}</span>
        <div className="home-table-art" aria-hidden="true">
          <div className="home-table-art__backdrop" />
          <span className="home-table-art__guest home-table-art__guest--left"><PlayerSilhouette color="#6aa9ff" /></span>
          <span className="home-table-art__guest home-table-art__guest--top"><PlayerSilhouette color="#50d4b0" /></span>
          <span className="home-table-art__guest home-table-art__guest--right"><PlayerSilhouette color="#ffbd73" /></span>
          <div className="home-table-art__cards"><span>A♠</span><span>K♥</span><span>Q♣</span></div>
        </div>
        <h2>{p.onFriends ? 'Играть с друзьями' : 'Собрать компанию'}</h2>
        <p>{p.onFriends ? '2–4 игрока · за одним столом' : 'Стол на 2–4 места с ботами'}</p>
        <button className="btn btn--primary btn--block" onClick={p.onResumeRoom ?? p.onFriends ?? p.onTable}>
          {p.onResumeRoom ? 'Вернуться за стол' : 'Создать стол'}
        </button>
      </section>

      <div className="home__actions">
        <button className="home-mode" onClick={p.hasSavedMatch ? p.onContinue : p.tutorialInProgress ? p.onContinueTutorial : p.onStart}>
          <QuickPlayIcon />
          <span><strong>{p.hasSavedMatch ? 'Продолжить матч' : p.tutorialInProgress ? 'Продолжить обучение' : 'Быстрая игра'}</strong><small>{opponentInfo(p.opponent).name} · один на один</small></span>
          <span aria-hidden="true">›</span>
        </button>
        {p.dailyCard}
        {p.onJoinByCode && <button className="home-mode home-mode--code" onClick={p.onJoinByCode}><span aria-hidden="true">⌘</span><strong>Войти по коду</strong><span aria-hidden="true">›</span></button>}
        <button className="home-more" onClick={() => setModes(true)}>Все режимы и выбор бота <span aria-hidden="true">›</span></button>
      </div>
      <nav className="home__secondary" aria-label="Об игре">
        <button className="btn btn--ghost" onClick={p.onStats}>История</button>
        <button className="btn btn--ghost" onClick={p.onRules}>Правила</button>
        <button className="btn btn--ghost" onClick={() => setSettings(true)}>Настройки</button>
      </nav>
      {played > 0 && <p className="home__record">Победы: {p.stats.wins} · Лучшая серия: {p.stats.bestStreak}</p>}
      {modes && <Dialog title="Режимы игры" onClose={() => setModes(false)}>
        <h2 className="surface__title">Режимы игры</h2>
        <div className="home__actions">
          <OpponentRow id={p.opponent} onClick={() => { setModes(false); p.onPickOpponent(); }} />
          {p.onFriends && <button className="btn btn--primary btn--block" onClick={() => { setModes(false); p.onFriends?.(); }}>Новый стол с друзьями</button>}
          <button className="btn btn--secondary btn--block" onClick={() => { setModes(false); p.onTable(); }}>{p.hasSavedTable ? 'Продолжить стол с ботами' : 'Стол с ботами · 2–4 места'}</button>
          {p.onDuel && <button className="btn btn--secondary btn--block" onClick={() => { setModes(false); p.onDuel?.(); }}>Дуэль один на один</button>}
          {p.hasSavedMatch && <button className="btn btn--secondary btn--block" onClick={() => { setModes(false); p.onRestart(); }}>Начать матч заново</button>}
          {p.tutorialInProgress && <button className="btn btn--secondary btn--block" onClick={() => { setModes(false); p.onSkipTutorial(); }}>Пропустить обучение</button>}
          <button className="btn btn--ghost btn--block" onClick={() => setModes(false)}>Закрыть</button>
        </div>
      </Dialog>}

      {settings && <Dialog title="Настройки" onClose={() => setSettings(false)}><h2 className="surface__title">Настройки</h2><FeedbackSettings /><button className="btn btn--primary btn--block" onClick={() => setSettings(false)}>Готово</button></Dialog>}
      <p className="home__legal">
        Только для развлечения: фишки виртуальные, не покупаются и не выводятся.
      </p>
    </main>
  );
}

function OpponentRow({ id, onClick }: { id: Opponent; onClick: () => void }) {
  const o = opponentInfo(id);
  return (
    <button className="opponent-row" onClick={onClick} aria-label={`Соперник: ${o.name}, ${o.style}. Сменить соперника`}>
      <BotAvatar color={o.color} />
      <span className="opponent-row__body">
        <span className="opponent-row__label">Соперник</span>
        <span className="opponent-row__name">{o.name}</span>
        <span className="opponent-row__meta">{o.style}</span>
        <Difficulty level={o.difficulty} />
      </span>
      <span className="opponent-row__change">Сменить</span>
    </button>
  );
}

/** Decorative participants: the social table is open to people, not only bots. */
function PlayerSilhouette({ color }: { color: string }) {
  return <svg viewBox="0 0 48 48" aria-hidden="true">
    <circle cx="24" cy="24" r="23" fill="#182c40" />
    <circle cx="24" cy="17" r="8" fill={color} />
    <path d="M9 40c0-9 6-14 15-14s15 5 15 14" fill={color} />
  </svg>;
}

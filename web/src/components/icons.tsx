// Inline SVG icons. Decorative unless a label is passed.

export function SpadeLogo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <path
        d="M32 6C23 19 10 26 10 38a11 11 0 0 0 18.5 8L25 58h14l-3.5-12A11 11 0 0 0 54 38C54 26 41 19 32 6Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="4.5"
        strokeLinejoin="round"
      />
      <path d="M36 18 25 37h7.5L28 51l12-20.5h-7.5L36 18Z" fill="var(--action-primary)" />
    </svg>
  );
}

export function HeroSpade() {
  return (
    <svg className="hero-spade" viewBox="0 0 120 120" aria-hidden="true">
      <defs>
        <linearGradient id="bolt" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#6A95FF" />
          <stop offset="1" stopColor="#2E5BD8" />
        </linearGradient>
      </defs>
      <path
        d="M60 10C44 33 20 45 20 67a20 20 0 0 0 33.5 14.8L47 106h26l-6.5-24.2A20 20 0 0 0 100 67C100 45 76 33 60 10Z"
        fill="#16233A"
        stroke="#fff"
        strokeWidth="7"
        strokeLinejoin="round"
      />
      <path d="M67 32 45 68h14l-8 26 23-38H60l7-24Z" fill="url(#bolt)" />
    </svg>
  );
}

/** Deliberately non-human avatar so it never reads as a real VK user. */
/** `color` tints the ring and the antenna light per opponent. */
export function BotAvatar({ color = '#6A95FF' }: { color?: string }) {
  return (
    <svg className="bot-avatar" viewBox="0 0 48 48" aria-hidden="true">
      <circle cx="24" cy="24" r="23" fill="#23395A" stroke={color} strokeWidth="2" />
      <rect x="13" y="16" width="22" height="17" rx="6" fill="#C3CFDA" />
      <circle cx="19.5" cy="24.5" r="2.6" fill="#101827" />
      <circle cx="28.5" cy="24.5" r="2.6" fill="#101827" />
      <path d="M24 16v-5" stroke="#C3CFDA" strokeWidth="2.4" strokeLinecap="round" />
      <circle cx="24" cy="10" r="2.4" fill={color} />
      <rect x="18" y="29" width="12" height="2" rx="1" fill="#101827" />
    </svg>
  );
}

export function Trophy() {
  return (
    <svg className="result-icon" viewBox="0 0 96 96" aria-hidden="true">
      <path d="M30 14h36v20a18 18 0 0 1-36 0V14Z" fill="#F5B83D" />
      <path
        d="M30 20H18a12 12 0 0 0 12 16M66 20h12a12 12 0 0 1-12 16"
        fill="none"
        stroke="#F5B83D"
        strokeWidth="5"
      />
      <path d="M43 52h10v14H43z" fill="#E0A12C" />
      <rect x="32" y="66" width="32" height="10" rx="3" fill="#F5B83D" />
      <path d="m48 21 3.5 7 7.5 1-5.5 5.3 1.4 7.7L48 38.3 41.1 42l1.4-7.7L37 29l7.5-1L48 21Z" fill="#fff" opacity=".85" />
    </svg>
  );
}

export function HandshakeIcon() {
  return (
    <svg className="result-icon" viewBox="0 0 96 96" aria-hidden="true">
      <circle cx="48" cy="48" r="34" fill="none" stroke="#C3CFDA" strokeWidth="5" />
      <path d="M30 48h36" stroke="#C3CFDA" strokeWidth="6" strokeLinecap="round" />
    </svg>
  );
}

export function CardsIcon() {
  return (
    <svg className="result-icon" viewBox="0 0 96 96" aria-hidden="true">
      <rect x="22" y="22" width="34" height="48" rx="5" fill="#23395A" stroke="#C3CFDA" strokeWidth="3" transform="rotate(-10 39 46)" />
      <rect x="40" y="24" width="34" height="48" rx="5" fill="#F8FAFC" />
      <path d="M57 36c-4 6-9 8-9 13a5 5 0 0 0 8.4 3.6L55 58h4l-1.4-5.4A5 5 0 0 0 66 49c0-5-5-7-9-13Z" fill="#101827" />
    </svg>
  );
}

export function ChipIcon() {
  return (
    <svg className="chip-icon" viewBox="0 0 16 16" aria-hidden="true">
      <ellipse cx="8" cy="11" rx="6.5" ry="2.6" fill="currentColor" opacity=".55" />
      <ellipse cx="8" cy="8" rx="6.5" ry="2.6" fill="currentColor" opacity=".8" />
      <ellipse cx="8" cy="5" rx="6.5" ry="2.6" fill="currentColor" />
    </svg>
  );
}

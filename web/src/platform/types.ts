// What the game needs from the messenger it runs in. VK, Telegram and a plain
// browser (guests by room code) implement it; the rest of the app never talks
// to a messenger SDK directly. All three share one duel server and one space
// of rooms: room codes work across platforms, invite links are per platform.

export type PlatformName = 'vk' | 'telegram' | 'browser';

export interface PlatformUser {
  name: string;
  photo: string | null;
}

/** Fields for POST /api/auth proving who the player is (verified by the server). */
export type AuthProof = { launch_params: string } | { tg_init_data: string };

export interface Platform {
  readonly name: PlatformName;
  /** Signed in through the messenger: may create rooms. Guests only join. */
  readonly canHost: boolean;
  /** Tell the messenger the app is ready (theme, viewport, splash). */
  init(): void;
  haptic(): void;
  /** Display name and avatar; null when the platform does not say. */
  getUser(): Promise<PlatformUser | null>;
  /** The messenger's share sheet; false when unavailable. */
  share(link: string): Promise<boolean>;
  copy(text: string): Promise<boolean>;
  /** Proof for sign-in; null: sign in as a guest. */
  authProof(): AuthProof | null;
  /** A link that opens this app straight in the room; null if not configured. */
  inviteLink(roomId: string): string | null;
  /** The room an invite opened the app in, if any (once). */
  launchRoom(): string | null;
  /** Forget the launch invite after it was used or left. */
  clearLaunchRoom(): void;
  /** Invites that arrive while the app is already open; returns unsubscribe. */
  onInvite(cb: (roomId: string) => void): () => void;
}

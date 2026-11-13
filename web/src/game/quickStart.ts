/** Start policy shared by normal quick play, rematches and the daily Play CTA.
 * A saved network game is resumed; disconnected games are never cloned locally. */
export function quickStartTarget(configured: boolean, authenticated: boolean, online: boolean, existingRoom: string | null): 'server' | 'resume' | 'local' {
  if (!configured || !authenticated || !online) return 'local';
  return existingRoom ? 'resume' : 'server';
}

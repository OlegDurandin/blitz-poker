// The platform this copy of the app runs in, decided once at start-up. One
// build serves VK, Telegram and plain browsers.

import { browser } from './browser';
import { isTelegram, telegram } from './telegram';
import type { Platform } from './types';
import { isVk, vk } from './vk';

function detect(): Platform {
  if (isTelegram()) return telegram;
  if (isVk()) return vk;
  return browser;
}

export const platform: Platform = detect();
export type { Platform, PlatformName, PlatformUser, AuthProof } from './types';

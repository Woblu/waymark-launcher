// Turns what the Rust side reports into a sentence a player can act on. The core
// sends a stable code with its message, so the code picks the sentence, and the
// message itself stays available for copying into a bug report.

import type { ApiError } from './api';

export interface Explained {
  code: string;
  /** What happened, in plain words. Empty when only the original message says it. */
  sentence: string;
  /** The message as the core wrote it. */
  technical: string;
  /** Whether the sentence says everything, so the original can stay out of sight. */
  complete: boolean;
}

function offlineNow(): boolean {
  return typeof navigator !== 'undefined' && navigator.onLine === false;
}

export function explain(error: unknown): Explained {
  const problem = (error ?? {}) as Partial<ApiError>;
  const code = typeof problem.code === 'string' ? problem.code : 'other';
  const technical = typeof problem.message === 'string' ? problem.message : error instanceof Error ? error.message : String(error);
  const say = (sentence: string, complete = true): Explained => ({ code, sentence, technical, complete });
  switch (code) {
    case 'network':
      if (offlineNow()) return say("You're offline. Check your connection and try again.");
      if (/timed? ?out/i.test(technical)) return say('The connection timed out. Try again in a moment.');
      return say("Couldn't reach the internet. Check your connection and try again.");
    case 'hash_mismatch':
      return say('A download was damaged on the way. Try again.');
    case 'http_status': {
      const status = Number(/HTTP (\d{3})/.exec(technical)?.[1] ?? 0);
      if (status === 401 && /forgecdn|curseforge/i.test(technical))
        return say("CurseForge refused the download. Update Waymark Launcher, or get the file from CurseForge's site.");
      if (status === 401) return say("The server didn't accept the sign-in. Sign in again, then try once more.");
      if (status === 403) return say('The server refused access to this.');
      if (status === 404) return say("The server couldn't find it. It may have been removed.");
      if (status === 429) return say('The server is getting too many requests. Wait a minute and try again.');
      if (status >= 500) return say('The server is having trouble right now. Try again in a few minutes.');
      return say('The server answered with an error. Try again later.');
    }
    case 'not_found':
      return say("Waymark Launcher couldn't find what it needed. It may have been moved or deleted.", false);
    case 'io':
      return say("Waymark Launcher couldn't read or write a file. Check that the disk has space and that no other program is using the file.", false);
    case 'format':
      return say("A file couldn't be read because it isn't in the format Waymark Launcher expected.", false);
    case 'archive':
      return say("A downloaded archive couldn't be opened. It may be damaged, so try again.", false);
    case 'app_not_approved':
      return say('Waymark Launcher is waiting on Mojang to approve it for Minecraft sign-in.');
    case 'not_owned':
      return say("This Microsoft account doesn't own Minecraft: Java Edition.");
    case 'auth':
      return say("Signing in with Microsoft didn't work. Try signing in again.", false);
    case 'sign_in_required':
      return say('Sign in with a Microsoft account that owns Minecraft: Java Edition first.');
    default:
      return say('', false);
  }
}

/** The core's own words, made to read as a sentence. */
export function asSentence(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return trimmed;
  const capital = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capital) ? capital : `${capital}.`;
}

/** Everything about a failure in one block of text, for pasting into a bug report. */
export function report(title: string, explained: Explained): string {
  return [title, explained.sentence, `${explained.code}: ${explained.technical}`].filter(Boolean).join('\n');
}

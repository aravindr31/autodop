/**
 * Client-side session credential helper (salted SHA-256 via WebCrypto).
 *
 * NOTE: this gates the local UI only — it is NOT real authentication. The
 * production credential check lives in the separately-built backend (the
 * Python app used bcrypt + Fernet against MongoDB). This gives the frontend a
 * working login/logout/change-password flow until that backend exists.
 */
import type { AuthCredential } from './types';

const encoder = new TextEncoder();

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Cryptographically random salt (16 bytes -> 32 hex chars). */
export function randomSalt(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return toHex(bytes);
}

/** Hash = SHA-256(salt + ":" + password). Async (WebCrypto). */
export async function hashPassword(salt: string, password: string): Promise<string> {
  const data = encoder.encode(`${salt}:${password}`);
  const digest = await crypto.subtle.digest('SHA-256', data);
  return toHex(new Uint8Array(digest));
}

export async function makeCredential(password: string): Promise<AuthCredential> {
  const salt = randomSalt();
  return { salt, hash: await hashPassword(salt, password) };
}

/** Constant-time-ish compare of two hex strings. */
export function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export async function verifyCredential(cred: AuthCredential | null, password: string): Promise<boolean> {
  if (!cred) return false;
  const candidate = await hashPassword(cred.salt, password);
  return timingSafeEqual(candidate, cred.hash);
}
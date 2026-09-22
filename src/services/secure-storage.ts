import { invoke } from '@tauri-apps/api/core';

interface EncryptedRecord {
  version: 1;
  iv: string;
  ciphertext: string;
  wrappedKey: string;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function toBase64(value: ArrayBuffer | Uint8Array) {
  return btoa(String.fromCharCode(...new Uint8Array(value)));
}

function fromBase64(value: string) {
  return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
}

export async function encryptSecureRecord<T>(value: T): Promise<EncryptedRecord> {
  const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, true, ['encrypt', 'decrypt']);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, encoder.encode(JSON.stringify(value)));
  const rawKey = await crypto.subtle.exportKey('raw', key);
  const wrappedKey = await invoke<number[]>('protect_secret', { secret: Array.from(new Uint8Array(rawKey)) });

  return { version: 1, iv: toBase64(iv), ciphertext: toBase64(ciphertext), wrappedKey: toBase64(new Uint8Array(wrappedKey).buffer) };
}

export async function decryptSecureRecord<T>(record: EncryptedRecord): Promise<T> {
  const unwrappedKey = await invoke<number[]>('unprotect_secret', { secret: Array.from(fromBase64(record.wrappedKey)) });
  const key = await crypto.subtle.importKey('raw', new Uint8Array(unwrappedKey), { name: 'AES-GCM' }, false, ['decrypt']);
  const plaintext = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: fromBase64(record.iv) }, key, fromBase64(record.ciphertext));
  return JSON.parse(decoder.decode(plaintext)) as T;
}

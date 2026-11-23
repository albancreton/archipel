/** Any JSON value. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type JsonObject = { [key: string]: Json };

export function isObject(value: Json | undefined): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Reads the value at a dotted path such as `subject.toolCall.toolCallId`. */
export function get(value: Json | undefined, path: string): Json | undefined {
  let current: Json | undefined = value;
  for (const key of path.split(".")) {
    if (!isObject(current)) return undefined;
    current = current[key];
  }
  return current;
}

/** Sets the value at a dotted path, creating objects along the way. */
export function set(object: JsonObject, path: string, value: Json): void {
  const keys = path.split(".");
  let current = object;
  for (const key of keys.slice(0, -1)) {
    const next = current[key];
    if (isObject(next)) {
      current = next;
    } else {
      const made: JsonObject = {};
      current[key] = made;
      current = made;
    }
  }
  current[keys[keys.length - 1]!] = value;
}

/** An id as a string, whether it is sent as a string or a number. */
export function idString(value: Json | undefined): string | undefined {
  if (typeof value === "string") return value;
  if (typeof value === "number") return String(value);
  return undefined;
}

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** Decodes standard base64. Returns undefined when the text is not base64. */
export function base64Decode(text: string): Uint8Array | undefined {
  const clean = text.replace(/=+$/, "");
  if (text.length % 4 !== 0 || /[^A-Za-z0-9+/]/.test(clean)) return undefined;
  const bytes = new Uint8Array(Math.floor((clean.length * 3) / 4));
  let buffer = 0;
  let bits = 0;
  let at = 0;
  for (const char of clean) {
    buffer = (buffer << 6) | ALPHABET.indexOf(char);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes[at++] = (buffer >> bits) & 0xff;
    }
  }
  return bytes;
}

export function base64Encode(bytes: Uint8Array): string {
  let out = "";
  for (let at = 0; at < bytes.length; at += 3) {
    const a = bytes[at]!;
    const b = bytes[at + 1];
    const c = bytes[at + 2];
    out += ALPHABET[a >> 2]!;
    out += ALPHABET[((a & 3) << 4) | ((b ?? 0) >> 4)]!;
    out += b === undefined ? "=" : ALPHABET[((b & 15) << 2) | ((c ?? 0) >> 6)]!;
    out += c === undefined ? "=" : ALPHABET[c & 63]!;
  }
  return out;
}

/** Decodes base64 text as UTF-8, for showing command output. */
export function base64Text(text: string): string {
  const bytes = base64Decode(text);
  return bytes ? new TextDecoder().decode(bytes) : text;
}

/**
 * The avatar rules the browser and the Gateway both have to agree on.
 *
 * This module is deliberately free of imports: it is the one part of the
 * contract package the browser loads at runtime, and a browser bundler
 * resolves the package as an external module, where the package's own
 * `.js`-suffixed specifiers would not map back to the TypeScript sources.
 * Keeping this file a leaf means the browser can import it directly.
 *
 * The Gateway's full avatar parser lives in `account.ts` and enforces the
 * same values, so the two sides cannot drift apart on what is acceptable.
 */

/** Largest avatar the Gateway accepts, in bytes of the encoded data URL. */
export const MAX_AVATAR_BYTES = 64 * 1024;

/** Image formats a browser can render from a data URL without a decoder. */
export const avatarMediaTypes = [
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
] as const;

export type AvatarMediaType = (typeof avatarMediaTypes)[number];

/** The UTF-8 byte length of a string, in both Node and the browser. */
export function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

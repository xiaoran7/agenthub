import { ContractError } from "./contract-error.js";
import { expectKeys, expectObject } from "./expect.js";
import {
  MAX_AVATAR_BYTES,
  avatarMediaTypes,
  utf8ByteLength,
  type AvatarMediaType,
} from "./avatar-rules.js";

/**
 * The user's own profile picture.
 *
 * The avatar is a data URL rather than a blob reference so the Gateway keeps
 * no file store: the console fetches the account and renders whatever comes
 * back. The size cap and the media-type allowlist are enforced here, because
 * this is the one place both the Gateway and the browser agree on them.
 *
 * This module runs in the browser as well as on the Gateway, so it uses
 * `atob` and `TextEncoder` rather than Node's `Buffer`. The shared values it
 * enforces live in `avatar-rules.ts`, which is a leaf module the browser can
 * import directly.
 */

export { MAX_AVATAR_BYTES, avatarMediaTypes, utf8ByteLength };
export type { AvatarMediaType };

export interface AvatarPayload {
  /** A `data:<media type>;base64,<payload>` URL. */
  data_url: string;
}

/**
 * Validate a submitted avatar.
 *
 * The caller hands over the raw data URL; anything that is not a base64 image
 * of an allowed type, or that is over the size cap, is rejected rather than
 * stored. A user can always clear the avatar by sending `null`.
 */
export function parseAvatarPayload(value: unknown): AvatarPayload | undefined {
  if (value === null || value === undefined) return undefined;
  const object = expectObject(value, "avatar");
  expectKeys(object, ["data_url"]);
  const dataUrl = object.data_url;
  if (typeof dataUrl !== "string") {
    throw new ContractError("invalid_field", "data_url must be a string");
  }
  if (dataUrl.length === 0) {
    throw new ContractError("invalid_field", "data_url must not be empty");
  }
  if (utf8ByteLength(dataUrl) > MAX_AVATAR_BYTES) {
    throw new ContractError(
      "avatar_too_large",
      `avatar must not exceed ${MAX_AVATAR_BYTES} bytes`,
    );
  }
  const match = /^data:([a-z]+\/[a-z0-9.+-]+);base64,([A-Za-z0-9+/=]+)$/.exec(
    dataUrl,
  );
  if (match === null) {
    throw new ContractError(
      "invalid_avatar",
      "avatar must be a base64 data URL",
    );
  }
  const mediaType = match[1] as string;
  if (!avatarMediaTypes.includes(mediaType as AvatarMediaType)) {
    throw new ContractError(
      "invalid_avatar",
      `avatar must be one of ${avatarMediaTypes.join(", ")}`,
    );
  }
  // The base64 body has to decode, otherwise the stored value is not an image
  // and the console would render a broken frame.
  const decoded = decodeBase64(match[2] as string);
  if (decoded.length === 0) {
    throw new ContractError("invalid_avatar", "avatar carries no image data");
  }
  return { data_url: dataUrl };
}

/** Decode a base64 string, in both Node and the browser. */
function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

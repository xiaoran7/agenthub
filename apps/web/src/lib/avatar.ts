/**
 * Turn a picked image file into an avatar data URL.
 *
 * The browser decodes and resizes the picture before it is sent, so the
 * Gateway receives a square thumbnail rather than the multi-megabyte original
 * a phone camera would produce. The Gateway still enforces the size cap: this
 * is a courtesy, not the authority.
 *
 * The cap and the media-type allowlist come from the shared contract, so the
 * browser and the Gateway cannot drift apart on what is acceptable.
 */

import {
  MAX_AVATAR_BYTES,
  avatarMediaTypes,
  utf8ByteLength,
} from "@agent-pulse/contracts/avatar-rules";

/** The edge length the avatar is scaled to. */
export const AVATAR_SIZE = 256;

const ALLOWED_TYPES = new Set<string>(avatarMediaTypes);

export class AvatarError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AvatarError";
  }
}

/**
 * Read a picked file into a square, downscaled data URL.
 *
 * The crop is centred: the middle of a picture is the part a person is
 * recognisable by, and a portrait is usually centred.
 */
export async function fileToAvatarDataUrl(file: File): Promise<string> {
  if (!ALLOWED_TYPES.has(file.type)) {
    throw new AvatarError("请选择 PNG、JPEG、WebP 或 GIF 格式的图片");
  }
  const source = await loadImage(file);
  const side = Math.min(source.width, source.height);
  const canvas = document.createElement("canvas");
  canvas.width = AVATAR_SIZE;
  canvas.height = AVATAR_SIZE;
  const context = canvas.getContext("2d");
  if (context === null) {
    throw new AvatarError("当前浏览器无法处理这张图片");
  }
  context.drawImage(
    source,
    (source.width - side) / 2,
    (source.height - side) / 2,
    side,
    side,
    0,
    0,
    AVATAR_SIZE,
    AVATAR_SIZE,
  );
  // PNG keeps transparency; the other formats would flatten it to black.
  const png = canvas.toDataURL("image/png");
  // A lossless PNG of a detailed picture easily passes the Gateway's cap even
  // at thumbnail size, so fall back to JPEG — which flattens transparency
  // against white — rather than rejecting an image the user could reasonably
  // expect to work.
  const dataUrl =
    utf8ByteLength(png) <= MAX_AVATAR_BYTES
      ? png
      : canvas.toDataURL("image/jpeg", 0.88);
  if (dataUrl.length === 0) {
    throw new AvatarError("当前浏览器无法处理这张图片");
  }
  // The JPEG fallback is not guaranteed to be smaller than the PNG, so it is
  // checked as well; otherwise the Gateway would reject it after the browser
  // had already accepted the picture.
  if (utf8ByteLength(dataUrl) > MAX_AVATAR_BYTES) {
    throw new AvatarError("这张图片太大了，请换一张小一点的");
  }
  return dataUrl;
}

function loadImage(file: File): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      URL.revokeObjectURL(url);
      resolve(image);
    };
    image.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new AvatarError("这张图片无法读取，请换一张试试"));
    };
    image.src = url;
  });
}

import { isIP } from "node:net";

function isForbiddenIpv4(a: number, b: number, c: number): boolean {
  return (
    a === 0 ||
    a === 10 ||
    a === 127 ||
    a >= 224 ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 192 && b === 0 && c === 0) ||
    (a === 198 && (b === 18 || b === 19)) ||
    (a === 100 && b >= 64 && b <= 127)
  );
}

function parseIpv6Groups(ip: string): number[] | null {
  const noZone = ip.split("%")[0]?.toLowerCase() ?? "";
  if (!noZone) return null;
  const dbl = noZone.split("::");
  if (dbl.length > 2) return null;
  const parseSide = (part: string): number[] | null => {
    if (!part) return [];
    const segs = part.split(":");
    const out: number[] = [];
    for (let i = 0; i < segs.length; i += 1) {
      const seg = segs[i]!;
      if (seg.includes(".")) {
        if (i !== segs.length - 1 || isIP(seg) !== 4) return null;
        const [a, b, c, d] = seg.split(".").map(Number);
        out.push(((a! << 8) | b!) >>> 0, ((c! << 8) | d!) >>> 0);
      } else if (/^[0-9a-f]{1,4}$/.test(seg)) {
        out.push(parseInt(seg, 16));
      } else {
        return null;
      }
    }
    return out;
  };
  const left = parseSide(dbl[0] ?? "");
  const right = dbl.length === 2 ? parseSide(dbl[1] ?? "") : [];
  if (!left || !right) return null;
  const missing = 8 - (left.length + right.length);
  if (
    (dbl.length === 1 && missing !== 0) ||
    (dbl.length === 2 && missing < 1)
  ) {
    return null;
  }
  return [...left, ...new Array<number>(missing).fill(0), ...right];
}

export function isForbiddenHost(hostname: string): boolean {
  const h = hostname
    .trim()
    .replace(/^\[|\]$/g, "")
    .split("%")[0]!
    .toLowerCase();
  if (
    !h ||
    h.endsWith(".") ||
    h === "localhost" ||
    h.endsWith(".localhost") ||
    h.endsWith(".local") ||
    h.endsWith(".internal")
  ) {
    return true;
  }
  const ipKind = isIP(h);
  if (ipKind === 4) {
    const [a, b, c] = h.split(".").map(Number);
    return isForbiddenIpv4(a!, b!, c!);
  }
  if (ipKind === 6 || h.includes(":")) {
    const g = parseIpv6Groups(h);
    if (!g) return true;
    const zeroHead = g.slice(0, 5).every((v) => v === 0);
    if (zeroHead && g[5] === 0 && g[6] === 0 && g[7]! <= 1) return true;
    if (zeroHead && (g[5] === 0xffff || g[5] === 0)) {
      return isForbiddenIpv4(g[6]! >> 8, g[6]! & 0xff, g[7]! >> 8);
    }
    // IPv4-translated ::ffff:0:0:0/96
    if (
      g[0] === 0 &&
      g[1] === 0 &&
      g[2] === 0 &&
      g[3] === 0 &&
      g[4] === 0xffff &&
      g[5] === 0
    ) {
      return true;
    }
    // NAT64 well-known prefix 64:ff9b::/96 and local-use 64:ff9b:1::/48
    if (
      g[0] === 0x0064 &&
      g[1] === 0xff9b &&
      (g[2] === 0x0001 ||
        (g[2] === 0 && g[3] === 0 && g[4] === 0 && g[5] === 0))
    ) {
      return true;
    }
    // Discard-only prefix 100::/64
    if (g[0] === 0x0100 && g[1] === 0 && g[2] === 0 && g[3] === 0) {
      return true;
    }
    const first = g[0]!;
    return (
      (first & 0xfe00) === 0xfc00 ||
      (first & 0xffc0) === 0xfe80 ||
      (first & 0xffc0) === 0xfec0 ||
      (first & 0xff00) === 0xff00
    );
  }
  if (!h.includes(".")) {
    return true;
  }
  return false;
}

export function isSafeHttpsRegistryUrl(raw: unknown): raw is string {
  if (typeof raw !== "string") return false;
  const trimmed = raw.trim();
  if (!trimmed || trimmed.length > 1024) return false;
  try {
    const u = new URL(trimmed);
    if (u.username || u.password) return false;
    if (process.env.AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY === "1") {
      return u.protocol === "https:" || u.protocol === "http:";
    }
    return u.protocol === "https:" && !isForbiddenHost(u.hostname);
  } catch {
    return false;
  }
}

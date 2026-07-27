const gatewayOrigin = process.env.GATEWAY_ORIGIN ?? "http://127.0.0.1:3001";

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // The workspace standardizes on TypeScript 7 (native compiler), which has
  // no in-process compiler API; let Next.js shell out to the TypeScript CLI.
  experimental: { useTypeScriptCli: true },
  async rewrites() {
    // Same-origin proxy to the gateway: the browser only ever talks to the
    // web app origin, so the gateway does not need CORS and x-user-id never
    // crosses origins. Production deployments can use a reverse proxy with
    // the same path layout instead.
    return [
      {
        source: "/v1/:path*",
        destination: `${gatewayOrigin}/v1/:path*`,
      },
    ];
  },
};

export default nextConfig;

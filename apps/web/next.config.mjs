/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // The workspace standardizes on TypeScript 7 (native compiler), which has
  // no in-process compiler API; let Next.js shell out to the TypeScript CLI.
  experimental: { useTypeScriptCli: true },
};

export default nextConfig;

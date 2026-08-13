/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Local QA and Android/Web device testing commonly use the loopback IP
  // instead of localhost; allow both without widening dev-resource origins.
  allowedDevOrigins: ["127.0.0.1", "localhost"],
  // The workspace standardizes on TypeScript 7 (native compiler), which has
  // no in-process compiler API; let Next.js shell out to the TypeScript CLI.
  experimental: { useTypeScriptCli: true },
};

export default nextConfig;

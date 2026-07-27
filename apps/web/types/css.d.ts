// TypeScript 7 does not pick up Next.js' bundled `*.css` module declaration
// through next-env.d.ts, so declare the side-effect import shape locally.
declare module "*.css";

// TypeScript does not resolve the side-effect CSS import shape on its own,
// so declare it locally for the bundler-facing entries.
declare module "*.css";

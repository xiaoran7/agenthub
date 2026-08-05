FROM node:22-bookworm-slim

WORKDIR /app

# Install from the lockfile before copying source so Docker can reuse the
# dependency layer when only application code changes.
COPY package.json package-lock.json tsconfig.base.json ./
COPY packages/contracts/package.json packages/contracts/package.json
COPY services/api/package.json services/api/package.json
COPY apps/web/package.json apps/web/package.json
RUN npm ci

COPY packages/contracts packages/contracts
COPY services/api services/api

ENV NODE_ENV=production

CMD ["sh", "-lc", "npm run migrate --workspace @agent-pulse/api && npm start --workspace @agent-pulse/api"]

FROM node:22-bookworm-slim

WORKDIR /app

# Install only production dependencies needed by @agent-pulse/api and its
# workspace contract package; tsx is a runtime dependency because contracts
# ships raw TypeScript entrypoints.
COPY package.json package-lock.json tsconfig.base.json ./
COPY packages/contracts/package.json packages/contracts/package.json
COPY services/api/package.json services/api/package.json
COPY apps/web/package.json apps/web/package.json
RUN npm ci --omit=dev --workspace @agent-pulse/api --workspace @agent-pulse/contracts --include-workspace-root \
 && mkdir -p /app/data /opt/data \
 && chown -R node:node /app /opt/data

COPY --chown=node:node packages/contracts packages/contracts
COPY --chown=node:node services/api services/api

ENV NODE_ENV=production
USER node

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD ["node", "-e", "fetch('http://127.0.0.1:' + (process.env.PORT || '3001') + '/health').then(r => process.exit(r.ok ? 0 : 1)).catch(() => process.exit(1))"]

CMD ["sh", "-c", "cd /app/services/api && if [ -n \"$AGENT_PLATFORM_DATABASE_URL\" ]; then ../../node_modules/.bin/tsx src/migrate.ts; fi && exec ../../node_modules/.bin/tsx src/server.ts"]

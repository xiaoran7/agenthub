FROM node:22-bookworm-slim

WORKDIR /app

COPY package.json package-lock.json tsconfig.base.json ./
COPY packages/contracts/package.json packages/contracts/package.json
COPY services/api/package.json services/api/package.json
COPY apps/web/package.json apps/web/package.json
RUN npm ci

COPY packages/contracts packages/contracts
COPY apps/web apps/web

RUN npm run build --workspace @agent-pulse/web

ENV NODE_ENV=production
ENV HOSTNAME=127.0.0.1
ENV PORT=3100

EXPOSE 3100

CMD ["npm", "run", "start", "--workspace", "@agent-pulse/web", "--", "-H", "127.0.0.1", "-p", "3100"]

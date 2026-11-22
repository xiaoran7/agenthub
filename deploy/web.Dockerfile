# Web 层：静态资源 + 同源 API 反代。
#
# 2.7.0 起 Web 层不再需要 Node/Next 运行时。原先这里跑的是 Next.js
# server（镜像 1.51GB、常驻约 65MB），现在只保留 nginx：
# 构建产物由 nginx 直接下发，镜像降到约 50MB、常驻约 10MB，
# 且 API 反代从 Next.js Route Handler 挪到了 nginx，Web 层不再有 JS 运行时。

FROM node:22-bookworm-slim AS builder

WORKDIR /app

# Install from the lockfile before copying source so Docker can reuse the
# dependency layer when only application code changes.
COPY package.json package-lock.json tsconfig.base.json ./
COPY packages/contracts/package.json packages/contracts/package.json
COPY services/api/package.json services/api/package.json
COPY apps/web/package.json apps/web/package.json
RUN npm ci

COPY packages/contracts packages/contracts
COPY apps/web apps/web

RUN npm run build --workspace @agent-pulse/web

FROM nginx:1.29-alpine

# Only GATEWAY_*/WEB_* are substituted at container start, so the nginx
# runtime variables ($host, $http_cookie, ...) are never mistaken for
# environment placeholders and blanked out.
ENV ENVSUBST_FILTER=^(GATEWAY_|WEB_)
ENV WEB_PORT=3100
ENV WEB_ROOT=/usr/share/nginx/html
ENV GATEWAY_ORIGIN=http://127.0.0.1:3101
ENV GATEWAY_ACCESS_TOKEN=""

# The stock default.conf listens on :80 and would collide with the host's
# own services under host networking. Writing the template to the same file
# name replaces it.
COPY deploy/nginx/agenthub-web.conf.template /etc/nginx/templates/default.conf.template
COPY --from=builder /app/apps/web/dist /usr/share/nginx/html
# Vite copies public files with their source modes. Let the nginx worker read
# them even if the build checkout used a restrictive umask.
RUN chmod -R a+rX /usr/share/nginx/html

EXPOSE 3100

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --quiet --tries=1 --spider "http://127.0.0.1:${WEB_PORT}/manifest.webmanifest" || exit 1

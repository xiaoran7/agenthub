# AgentHub 部署说明 (DEPLOYMENT)

Gateway API（`:3001`）与 Next.js Web（`:3000`）支持两种持久化部署模式，均由 Web 同源代理 `/v1` 并通过反向代理（Nginx / Caddy / OpenResty）对外提供 HTTPS。

> **安全红线**：严禁将 Gateway API 端口或数据库端口直接暴露公网；严禁在公网启用 `AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true`；严禁将内部访问令牌写入前端构建参数、URL 或 `localStorage`。

## 1. 极简自部署（单机 SQLite，零外部数据库依赖）

默认使用 Node.js 22 原生 `node:sqlite` 将账户/设备持久化至 `gateway.db`，将 Remote v3 内容持久化至 `remote.db`：

```bash
cp deploy/docker.env.selfhost.example deploy/docker.env && chmod 600 deploy/docker.env
# 编辑 deploy/docker.env，设置至少 32 字节的 GATEWAY_ACCESS_TOKEN
docker compose --env-file deploy/docker.env -f deploy/docker-compose.selfhost.yml up -d --build
```

## 2. 生产级部署（PostgreSQL + SQLite Remote 库）

复制 `deploy/agent-pulse.env.example`（裸机 `/etc/agent-pulse/agent-pulse.env`）或 `deploy/docker.env.example`（Docker / 1Panel），权限设为 `0600`：

- **核心环境变量**：
  - `AGENT_PLATFORM_DATABASE_URL`：PostgreSQL 连接串（未设置时自动使用 `AGENTHUB_GATEWAY_DATABASE`，默认 `./data/gateway.db`）
  - `AGENTHUB_REMOTE_DATABASE`：Remote v3 单实例 SQLite 路径（默认 `./data/remote.db`）
  - `AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET`：`node -e "console.log(require('node:crypto').randomBytes(32).toString('base64url'))"`
  - `GATEWAY_ORIGIN=http://127.0.0.1:3001`
- **可选扩展（按需成组配置，未配置时对应功能安全禁用）**：
  - **邮件（二选一）**：HTTP 邮件适配器（`AGENT_PULSE_PUBLIC_BASE_URL` + `AGENT_PULSE_EMAIL_ENDPOINT` + `AGENT_PULSE_EMAIL_TOKEN`）或 Resend（`AGENT_PULSE_PUBLIC_BASE_URL` + `AGENT_PULSE_RESEND_API_KEY` + `AGENT_PULSE_RESEND_FROM`）；
  - **Stripe 订阅**：`AGENT_PULSE_STRIPE_SECRET_KEY`、`AGENT_PULSE_STRIPE_WEBHOOK_SECRET`、`AGENT_PULSE_STRIPE_PRICE_PRO`、`AGENT_PULSE_STRIPE_PRICE_TEAM`、`AGENT_PULSE_PUBLIC_BASE_URL`；
  - **Web Push**：`npm run vapid-keys --workspace @agent-pulse/api` 生成三元组。

### 裸机 Systemd 启动

```bash
npm ci
sudo -u agentpulse env $(grep -v '^#' /etc/agent-pulse/agent-pulse.env | xargs) \
  npm run migrate --workspace @agent-pulse/api
npm run build --workspace @agent-pulse/web
sudo cp deploy/systemd/agent-pulse-api.service deploy/systemd/agent-pulse-web.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now agent-pulse-api agent-pulse-web
```

### Docker / 1Panel 启动（Host Network 绑定 `127.0.0.1:3100` / `3101`）

```bash
cp deploy/docker.env.example deploy/docker.env && chmod 600 deploy/docker.env
docker compose --env-file deploy/docker.env -f deploy/docker-compose.production.yml up -d --build
```

## 3. HTTPS 反向代理与上线核验

参考 `deploy/nginx/agent-pulse.conf.example` 或 `deploy/1panel/platform.tannerlab.xyz.conf.example` 配置 TLS 反向代理：

- **流式通道要求**：`/v1/stream`（SSE）与 `/v3/remote/ws`（WebSocket）必须关闭代理缓冲（`proxy_buffering off`）并保留长读超时。
- **上线验收**：
  1. `curl -fsS http://127.0.0.1:3001/health` 返回 `{"status":"ok"}`；
  2. 访问 `https://your-domain/` 完成首次单管理员初始化（或口令登录），未授权直访 `/v1/devices` 返回 401；
  3. 本机 `agenthub-runner` 通过 HTTPS/WSS 完成一次性配对码绑定，流式输出、审批与中断实时同步，重启服务后数据不丢失。

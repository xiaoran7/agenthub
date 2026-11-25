# AgentHub 部署说明 (DEPLOYMENT)

Gateway API（`@agent-pulse/api`）与静态 Web 前门（Vite 8 构建产物 + `nginx:1.29-alpine`）支持三种标准拓扑。Web 前门同时承载静态托管与 `/v1/*`、`/v3/remote/*`、`/health`、`/healthz` 同源反代，外层 TLS 反向代理只需指向 Web 端口。

| 部署模式 | API 监听 | Web 监听 | 配置文件 |
| :--- | :--- | :--- | :--- |
| **极简自部署（Docker 共享网络命名空间）** | `127.0.0.1:3001` | `0.0.0.0:3000` | `deploy/docker-compose.selfhost.yml` |
| **生产容器（Docker Host 网络）** | `127.0.0.1:3101` | `127.0.0.1:3100` | `deploy/docker-compose.production.yml` |
| **裸机服务（Systemd + Nginx）** | `127.0.0.1:3001` | `127.0.0.1:3000` | `deploy/agent-pulse.env.example`、`deploy/systemd/` |

> **安全红线**：严禁将 Gateway API 或数据库端口直接暴露公网；严禁在公网开启 `AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true`；严禁把网关内部访问令牌打入前端构建产物或 URL。

## 1. 极简自部署（纯 SQLite，零外部数据库依赖）

默认以 Node.js 22 内置 `node:sqlite` 持久化账户库 `gateway.db` 与 Remote 内容库 `remote.db`：

```bash
cp deploy/docker.env.selfhost.example deploy/docker.env && chmod 600 deploy/docker.env
# 编辑 deploy/docker.env，配置 >=32 字节随机 base64url 串 AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET
docker compose --env-file deploy/docker.env -f deploy/docker-compose.selfhost.yml up -d --build
```

Web 与 API 共享网络命名空间。宿主机只发布 Web 的 3000 端口，Web 通过容器内的回环地址连接 API。

> **Bearer 兼容模式警告**：默认采用账号登录与 HttpOnly Cookie 会话鉴权。若取消注释 `AGENT_PLATFORM_ACCESS_TOKEN` 与 `GATEWAY_ACCESS_TOKEN` 启用单用户无账号 Bearer 模式，Web 容器会自动为所有代理请求注入 Bearer 令牌，任何能直连 3000 端口的客户端都将获得完整控制权。启用该模式时，**必须**将 `deploy/docker-compose.selfhost.yml` 的端口映射收紧为 `"127.0.0.1:3000:3000"`，并仅经由带独立鉴权的外层反向代理或私有 VPN 访问。

## 2. 生产部署（PostgreSQL 元数据 + SQLite Remote 库）

按 `deploy/docker.env.example`（或裸机 `/etc/agent-pulse/agent-pulse.env`，权限 `0600`）配置：
- **核心变量**：`AGENT_PLATFORM_DATABASE_URL`（PostgreSQL 连接串）、`AGENTHUB_REMOTE_DATABASE`（容器默认 `/opt/data/remote.db`，Systemd 默认 `/var/lib/agent-pulse/remote.db`）、`AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET`、`AGENTHUB_RUNNER_OFFLINE_GRACE_MS`（默认 `6000`）。生产形态使用账户 HttpOnly Cookie 鉴权，**不要**设置 `GATEWAY_ACCESS_TOKEN` 或 `AGENT_PLATFORM_ACCESS_TOKEN`。
- **可选能力**（未配置自动安全禁用）：邮件通知（HTTP 适配器或 Resend）、Stripe 订阅 Webhook、VAPID Web Push（`npm run vapid-keys --workspace @agent-pulse/api`）。

```bash
# Docker Host 网络模式（监听 127.0.0.1:3100 / 3101）
cp deploy/docker.env.example deploy/docker.env && chmod 600 deploy/docker.env
docker compose --env-file deploy/docker.env -f deploy/docker-compose.production.yml up -d --build
```

裸机 Systemd 模式：
1. 创建隔离账号：`useradd -r -s /usr/sbin/nologin agentpulse`（API）与 `useradd -r -s /usr/sbin/nologin agentpulse-web`（Web/Nginx）；
2. 运行 `npm ci && npm run migrate --workspace @agent-pulse/api && npm run build --workspace @agent-pulse/web`，将 `apps/web/dist/.` 拷至 `/opt/agent-pulse/web/`（保持 `/opt/agent-pulse` 对服务只读）；
3. 配置 `/etc/agent-pulse/agent-pulse.env`（`0600`，属主 `agentpulse`），如需覆盖 Web 端口或网关地址请单独创建 `/etc/agent-pulse/web.env`（`0640`，属组 `agentpulse-web`，严禁与前端共享后端密钥）；
4. 安装 `deploy/systemd/agent-pulse-{api,web}.service` 与 `deploy/systemd/nginx.conf`（由 systemd `StateDirectory` / `RuntimeDirectory` / `LogsDirectory` / `CacheDirectory` 自动托管可写目录）后启用服务。

## 3. HTTPS 反向代理与上线核验

代理信任策略（与 `services/api/src/security-guards.ts` 保持一致）：
- `AGENT_PULSE_TRUST_PROXY_HOPS=0` 显式关闭转发头信任（`false`）；
- 配置 `AGENT_PULSE_TRUST_PROXY_ADDRESSES`（逗号分隔的代理 IP 或 CIDR）时按白名单信任，并优先于正整数 `AGENT_PULSE_TRUST_PROXY_HOPS`；
- 未配置 `AGENT_PULSE_TRUST_PROXY_ADDRESSES` 时，正整数 `AGENT_PULSE_TRUST_PROXY_HOPS`（`1..8`）直接设置 Fastify 可信代理跳数（跨主机反代建议优先用 `AGENT_PULSE_TRUST_PROXY_ADDRESSES` 限定源地址）；
- 两项均未配置时，监听回环地址默认信任 `127.0.0.1/8` 与 `::1/128`，非回环监听默认 `false`。

参考 `deploy/nginx/agent-pulse.conf.example` 或 `deploy/1panel/platform.example.com.conf.example`：
1. `/v1/stream`（SSE）与 `/v3/remote/ws`（WebSocket）必须关闭缓冲（`proxy_buffering off`）并透传 `Upgrade` / `Connection` 与 `X-Forwarded-Proto`；
2. 探针校验：`curl -fsS http://127.0.0.1:3101/health` 与 `curl -fsS http://127.0.0.1:3100/health` 返回 `{"status":"ok"}`，`curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:3100/` 返回 `200`，匿名访问 `/v1/devices` 返回 `401`；
3. 可运行 `npm run test:proxy` 在容器内执行同源反代安全、Cookie 白名单与缓存头契约回归。

## 4. Windows Runner 开机自启

Runner 编译为 Windows GUI 子系统（无黑框，无需 `.vbs` 包装），推荐在 `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\` 放置快捷方式 `AgentHub Runner.lnk`：
- **目标参数**：`agenthub-runner.exe web --port 4200 --open-browser false --hide-console true`（起始位置设为真实存在的工作区目录）。
- **单实例与退出**：内置命名管道单实例互斥，重复启动自动安全退出；通过托盘菜单 `"退出 AgentHub"` 或 `POST /api/shutdown` 可连同 `ui` 子进程树干净关闭。

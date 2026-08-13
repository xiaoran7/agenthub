# AgentPulse 单机上线说明

这份方案适用于一台 Linux 服务器上的个人部署：PostgreSQL 持久化数据，Gateway API 只监听本机，Next.js Web 通过同源 `/v1` 代理访问 API，Nginx 或 Caddy 对外提供 HTTPS。

不要把 Gateway API 端口直接暴露到公网，也不要把 `GATEWAY_ACCESS_TOKEN` 写进浏览器环境变量、URL、localStorage 或前端代码。

## 1. 服务器准备

- Node.js 22 或更高版本
- PostgreSQL
- Nginx 或 Caddy，用于 TLS 终止和反向代理
- 一个已经解析到服务器的域名

在服务器上准备目录和专用用户，例如：

```bash
sudo useradd --system --home /opt/agent-pulse --shell /usr/sbin/nologin agentpulse
sudo mkdir -p /opt/agent-pulse /etc/agent-pulse
sudo chown -R agentpulse:agentpulse /opt/agent-pulse
```

创建 PostgreSQL 数据库和用户，并确保数据库只允许可信网络访问：

```sql
CREATE USER agent_pulse WITH PASSWORD 'replace-with-a-long-password';
CREATE DATABASE agent_pulse OWNER agent_pulse;
```

## 2. 获取代码并安装依赖

以 `agentpulse` 用户执行：

```bash
cd /opt/agent-pulse
git clone <repository-url> .
npm ci
```

生产部署必须提交并使用锁文件；更新版本时使用 `git fetch`、切换到明确的提交，再重新执行 `npm ci`。

## 3. 配置密钥

```bash
sudo install -o agentpulse -g agentpulse -m 600 \
  deploy/agent-pulse.env.example /etc/agent-pulse/agent-pulse.env
sudoedit /etc/agent-pulse/agent-pulse.env
```

至少替换这些值：

- `AGENT_PLATFORM_DATABASE_URL`
- `AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET`
- `GATEWAY_ORIGIN=http://127.0.0.1:3001`

用 `node -e "console.log(require('node:crypto').randomBytes(32).toString('base64url'))"`
生成登录限流密钥。需要邮箱验证时成组设置 `AGENT_PULSE_PUBLIC_BASE_URL`、
`AGENT_PULSE_EMAIL_ENDPOINT`、`AGENT_PULSE_EMAIL_TOKEN`；账单适配器使用独立的
`AGENT_PULSE_BILLING_WEBHOOK_SECRET`。这些值只进入服务器密钥文件，不进入 Web 构建参数。

新部署使用用户注册/登录和 HttpOnly 会话 Cookie，不配置全局 owner 或访问令牌。旧个人
部署迁移时才使用示例文件里注释的 Bearer 兼容变量。

旧兼容模式随机令牌示例：

```bash
node -e "console.log(require('node:crypto').randomBytes(32).toString('base64url'))"
```

不要设置 `AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true`。它只允许回环地址上的本地开发身份，不适用于上线服务器。

## 4. 数据库迁移与生产构建

```bash
sudo -u agentpulse env $(grep -v '^#' /etc/agent-pulse/agent-pulse.env | xargs) \
  npm run migrate --workspace @agent-pulse/api
npm run typecheck
npm test
npm run build --workspace @agent-pulse/web
```

迁移脚本是幂等的；systemd 示例会在 API 每次启动前再次执行迁移。

## 5. 启动服务

仓库已提供：

- `deploy/systemd/agent-pulse-api.service`
- `deploy/systemd/agent-pulse-web.service`

复制并启用：

```bash
sudo cp deploy/systemd/agent-pulse-api.service /etc/systemd/system/
sudo cp deploy/systemd/agent-pulse-web.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now agent-pulse-api agent-pulse-web
```

检查：

```bash
curl -fsS http://127.0.0.1:3001/health
curl -fsS http://127.0.0.1:3000/ | head
sudo journalctl -u agent-pulse-api -u agent-pulse-web -f
```

API 应返回 `{"status":"ok"}`。如果 API 因为令牌不一致启动成功但 Web 显示网关错误，先核对两个 token；浏览器请求本身不应携带 Gateway Bearer token。

## 6. 配置 HTTPS 反向代理

复制 `deploy/nginx/agent-pulse.conf.example`，替换域名和证书路径后启用：

```bash
sudo nginx -t
sudo systemctl reload nginx
```

`/v1/stream` 必须关闭代理缓冲，并保留较长的读取超时，否则 SSE 会表现为“已连接但没有实时更新”。Web Push 也要求页面通过 HTTPS（本地 `localhost` 除外）。

上线后的验收顺序：

1. `https://your-domain/` 能加载中文面板。
2. 面板右上角显示“网关已连接”或实时连接状态。
3. `https://your-domain/v1/devices` 在未携带浏览器 token 时不会绕过 Web 代理泄露数据；通过页面加载应能正常返回数据。
4. Runner 使用 HTTPS Gateway 地址完成设备绑定和同步。
5. 新事件写入后，Sessions 和 Activity 页面能通过 SSE 刷新；若 SSE 不可用，页面应退回轮询。
6. 重启 API 后历史设备和 Session 仍存在，证明使用的是 PostgreSQL 而非内存 Store。
7. 请求密码重置邮件，确认链接片段在页面载入后立即从地址栏移除；重置后旧密码和旧
   Web 会话全部失效，新密码可登录。

## 7. 当前上线边界

当前已支持多用户邮箱账户、Web 会话轮换、邮箱验证、密码重置与签名订阅事件，但仍不
包含 SSO、真实邮件/支付供应商适配、跨实例事件广播和完整分布式限流。正式对公网开放
前，需要接入供应商并在反向代理或网关层补充全局限流、访问日志脱敏和备份恢复策略。

## 8. Docker 部署

如果服务器没有 Node.js 22，可以使用仓库内的 Docker 方案。它让 API 和 Web 使用 host network，只绑定本机 `3101` 和 `3100`，因此仍然满足同源代理的安全约束。生产数据库建议交给 1Panel 应用商店管理，API 通过本机 `5432` 连接它。

```bash
cp deploy/docker.env.example deploy/docker.env
chmod 600 deploy/docker.env
# 编辑 deploy/docker.env，确保数据库密码和连接串一致，并设置随机访问令牌
docker compose --env-file deploy/docker.env \
  -f deploy/docker-compose.production.yml up -d --build
docker compose --env-file deploy/docker.env \
  -f deploy/docker-compose.production.yml ps
```

Docker 部署时，反向代理目标仍是 `http://127.0.0.1:3100`；不要把 3100、3101 或 5432 改成公网监听。

## 9. 1Panel 部署

先在 1Panel 应用商店安装 PostgreSQL，再在数据库页面创建 `agent_pulse` 数据库和同名应用用户，并限制为本地访问。然后使用 `deploy/docker-compose.production.yml` 作为 AgentPulse 的编排文件；该编排只启动 API 和 Web，不再重复创建 PostgreSQL。推荐把项目放在 `/opt/1panel/apps/agent-pulse`，把密钥放在 `deploy/docker.env`，将 `AGENT_PLATFORM_DATABASE_URL` 指向 `127.0.0.1:5432`，再从 1Panel 的容器/编排界面执行构建和启动。

`deploy/1panel/platform.tannerlab.xyz.conf.example` 是 OpenResty 站点配置：它把域名转发到本机 3100，并对 `/v1/stream` 关闭缓冲。若服务器已有 `*.tannerlab.xyz` 通配符证书，可直接在 1Panel 站点配置中复用；否则先用 1Panel 申请该域名证书，再替换配置中的证书路径。

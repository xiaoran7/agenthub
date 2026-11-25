#!/usr/bin/env bash
# Web 前门（nginx）同源代理契约测试。
#
# 这份脚本承担 2.7.0 之前 apps/web/test/gateway-proxy.test.ts 的职责。
# 旧测试断言的是 Next.js Route Handler 代理的语义；迁移到 nginx 之后，
# 这些语义由 deploy/nginx/agenthub-web.conf.template 承载，单元测试
# 已经测不到，必须对真实 nginx 做实测。
#
# 覆盖的契约：
#   1. 会话 Cookie 白名单：仅大小写敏感放行 43 位 base64url 的 agent_pulse_session / __Host-agent_pulse_session；
#   2. 服务端令牌注入：配置 GATEWAY_ACCESS_TOKEN 时覆盖浏览器自带的 Authorization；
#   3. 未配置令牌时：Authorization 整条丢弃，浏览器凭据无法直连网关；
#   4. X-User-Id 与 Forwarded：无论是否配置令牌均剥离，防止身份伪造与代理头走私；
#   9. 健康检查：/health 与 /healthz 透传至上游网关并下发 no-store。
#   5. X-Forwarded-Proto：优先沿用外层反代传入的协议（如 https）；
#   6. 缓存策略：动态 API 下发 no-store，子页面 HTML 下发 no-cache；
#   7. /v3 下非 remote 前缀返回 404 JSON；
#   8. 上游不可达返回 502 {"error":"gateway_unreachable"}。
#
# 用法：bash scripts/test-web-proxy.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$ROOT/deploy/nginx/agenthub-web.conf.template"
IMAGE="${NGINX_IMAGE:-nginx:1.29-alpine}"
UPSTREAM_IMAGE="${UPSTREAM_IMAGE:-node:22-alpine}"
TOKEN="test-access-token-32-bytes-long!!"
VALID_PLAIN_SESSION="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklm0123"
VALID_HOST_SESSION="zyxwvutsrqponmlkjihgfedcbaZYXWVUTSRQPON_-09"

NET="agenthub-proxy-test-$$"
TMP_DIR="/tmp/agenthub-nginx-tmpl-$$"

cleanup() {
  docker rm -f "$NET-web" "$NET-web-dead" "$NET-upstream" >/dev/null 2>&1 || true
  docker network disconnect -f "$NET" "$(hostname)" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# 读取回声上游吐回来的请求头
upstream_headers() {
  curl -s --max-time 10 "$@"
}

assert_eq() {
  local label="$1" actual="$2" expected="$3"
  if [ "$actual" != "$expected" ]; then
    fail "$label：期望 [$expected]，实际 [$actual]"
  fi
  echo "  ok  $label = $actual"
}

# 辅助函数：启动 nginx 测试容器
# 规避 DooD（Docker-out-of-Docker）环境下容器内路径挂载到宿主机不存在的问题，改用 docker cp 注入模板
start_web_container() {
  local name="$1"
  local origin="$2"
  local token="$3"

  docker create --name "$name" --network "$NET" \
    -e WEB_HOST=0.0.0.0 \
    -e WEB_PORT=8080 \
    -e WEB_ROOT=/usr/share/nginx/html \
    -e GATEWAY_ORIGIN="$origin" \
    -e GATEWAY_ACCESS_TOKEN="$token" \
    -e 'ENVSUBST_FILTER=^(GATEWAY_|WEB_)' \
    "$IMAGE" >/dev/null

  docker cp "$TMP_DIR/templates" "$name:/etc/nginx/"
  docker start "$name" >/dev/null
}

wait_for_web_container() {
  local name="$1"
  local ip=""
  for _ in $(seq 1 30); do
    ip="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name" 2>/dev/null || true)"
    if [ -n "$ip" ] && [ "$ip" != "invalid IP" ] && \
       curl -s --max-time 2 -o /dev/null "http://$ip:8080/manifest.webmanifest" 2>/dev/null; then
      echo "$ip"
      return 0
    fi
    sleep 0.5
  done
  docker logs "$name" >&2 || true
  fail "等待测试容器 $name 就绪超时"
}

echo "== 准备环境 =="
mkdir -p "$TMP_DIR/templates"
cp "$TEMPLATE" "$TMP_DIR/templates/default.conf.template"

docker network create "$NET" >/dev/null
# 若当前在容器内执行（如 Gitea act-runner / GitHub Actions），将自身容器接入测试网桥以支持直连容器 IP
docker network connect "$NET" "$(hostname)" >/dev/null 2>&1 || true

docker run -d --name "$NET-upstream" --network "$NET" \
  "$UPSTREAM_IMAGE" node -e \
  "require('http').createServer((q,s)=>{s.setHeader('content-type','application/json');s.end(JSON.stringify({path:q.url,headers:q.headers}))}).listen(3999)" \
  >/dev/null

# 等待上游分配 IP 并就绪，直接用 IP 访问避免嵌入式 DNS 受宿主机 search 域（如 Tailscale）干扰
UPSTREAM_IP=""
UPSTREAM_READY=0
for _ in $(seq 1 30); do
  UPSTREAM_IP="$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$NET-upstream" 2>/dev/null || true)"
  if [ -n "$UPSTREAM_IP" ] && [ "$UPSTREAM_IP" != "invalid IP" ] && \
     curl -s --max-time 2 -o /dev/null "http://$UPSTREAM_IP:3999/health" 2>/dev/null; then
    UPSTREAM_READY=1
    break
  fi
  sleep 0.5
done
[ "$UPSTREAM_READY" -eq 1 ] || fail "上游回声容器未能就绪"

start_web_container "$NET-web" "http://$UPSTREAM_IP:3999" "$TOKEN"
WEB_IP="$(wait_for_web_container "$NET-web")"
WEB="http://$WEB_IP:8080"

echo "== 契约 1-2：令牌注入 + 43 位会话 Cookie 白名单 + 剥离伪造头 =="
JSON="$(upstream_headers \
  -H "Authorization: Bearer forged-by-browser" \
  -H "Cookie: tracker=evil; agent_pulse_session=$VALID_PLAIN_SESSION; other=1" \
  -H "X-User-Id: spoofed-user" \
  -H "Forwarded: for=1.2.3.4;proto=https" \
  "$WEB/v1/auth/me")"

assert_eq "上游收到的 authorization" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("authorization",""))')" \
  "Bearer $TOKEN"
assert_eq "上游收到的 cookie" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("cookie",""))')" \
  "agent_pulse_session=$VALID_PLAIN_SESSION"
assert_eq "上游收到的 x-user-id（配置令牌时应被丢弃）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-user-id",""))')" \
  ""
assert_eq "上游收到的 forwarded（应被剥离）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("forwarded",""))')" \
  ""

echo "== 契约 1：__Host- 优先于无前缀 Cookie，且严格校验大小写与 43 位 base64url =="
JSON="$(upstream_headers -H "Cookie: agent_pulse_session=$VALID_PLAIN_SESSION; __Host-agent_pulse_session=$VALID_HOST_SESSION; junk=2" "$WEB/v1/auth/me")"
assert_eq "上游收到的 cookie（__Host- 优先）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("cookie",""))')" \
  "__Host-agent_pulse_session=$VALID_HOST_SESSION"

for bad_cookie in \
  "AGENT_PULSE_SESSION=$VALID_PLAIN_SESSION" \
  "__host-agent_pulse_session=$VALID_HOST_SESSION" \
  "agent_pulse_session=SHORT" \
  "agent_pulse_session=${VALID_PLAIN_SESSION}X" \
  "agent_pulse_session=ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklm012="; do
  JSON_BAD="$(upstream_headers -H "Cookie: $bad_cookie" "$WEB/v1/auth/me")"
  assert_eq "拒绝非法/错大小写/非 43 位 cookie ($bad_cookie)" \
    "$(echo "$JSON_BAD" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("cookie",""))')" \
    ""
done

echo "== 契约 4：Cookie 模式下浏览器 Authorization/X-User-Id/Forwarded 均不得直通网关 =="
start_web_container "$NET-web-dead" "http://$UPSTREAM_IP:3999" ""
DEAD_IP="$(wait_for_web_container "$NET-web-dead")"
JSON="$(upstream_headers \
  -H "Authorization: Bearer forged-by-browser" \
  -H "Cookie: tracker=evil; agent_pulse_session=$VALID_PLAIN_SESSION" \
  -H "X-User-Id: dev-user" \
  -H "Forwarded: for=1.2.3.4" \
  "http://$DEAD_IP:8080/v1/auth/me")"

assert_eq "上游收到的 authorization（应为空）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("authorization",""))')" \
  ""
assert_eq "上游收到的 x-user-id（Cookie 模式默认剥离）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-user-id",""))')" \
  ""
assert_eq "上游收到的 forwarded（应为空）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("forwarded",""))')" \
  ""
assert_eq "上游收到的 cookie" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("cookie",""))')" \
  "agent_pulse_session=$VALID_PLAIN_SESSION"

echo "== 契约 3：X-Forwarded-Proto 与 X-Forwarded-Host 透传 =="
JSON="$(upstream_headers -H "Host: platform.example.com" -H "X-Forwarded-Proto: https" "$WEB/v1/auth/me")"
assert_eq "上游收到的 x-forwarded-proto（外层为 https 时保持 https）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-forwarded-proto",""))')" \
  "https"
assert_eq "上游收到的 x-forwarded-host（默认继承外层 Host）" \
  "$(echo "$JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-forwarded-host",""))')" \
  "platform.example.com"

JSON_HTTP="$(upstream_headers "$WEB/v1/auth/me")"
assert_eq "上游收到的 x-forwarded-proto（未传时回落到当前 scheme: http）" \
  "$(echo "$JSON_HTTP" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-forwarded-proto",""))')" \
  "http"

echo "== 契约 缓存策略：API 禁用缓存，子页面 HTML 强校验 =="
API_CC="$(curl -s -I "$WEB/v1/auth/me" | tr -d "\r" | grep -i "^cache-control:" | awk '{print $2}')"
assert_eq "API 响应 Cache-Control" "$API_CC" "no-store"

docker exec "$NET-web" mkdir -p /usr/share/nginx/html/remote
docker exec "$NET-web" touch /usr/share/nginx/html/remote/index.html
REMOTE_CC="$(curl -s -I "$WEB/remote" | tr -d "\r" | grep -i "^cache-control:" | awk '{print $2}')"
assert_eq "/remote 子页面 Cache-Control" "$REMOTE_CC" "no-cache"

echo "== 契约 5：/v3 非 remote 前缀返回 404 JSON =="
BODY="$(curl -s --max-time 10 "$WEB/v3/foo")"
assert_eq "/v3/foo 响应体" "$BODY" '{"error":"not_found"}'

echo "== 契约 7：/health 与 /healthz 透传至网关并禁用缓存 =="
for hpath in "/health" "/healthz"; do
  HJSON="$(upstream_headers -H "X-User-Id: spoofed" "$WEB$hpath")"
  assert_eq "$hpath 透传至上游路径" \
    "$(echo "$HJSON" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("path",""))')" \
    "$hpath"
  assert_eq "$hpath 继承剥离 x-user-id" \
    "$(echo "$HJSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["headers"].get("x-user-id",""))')" \
    ""
  HCC="$(curl -s -I "$WEB$hpath" | tr -d "\r" | grep -i "^cache-control:" | awk '{print $2}')"
  assert_eq "$hpath 响应 Cache-Control" "$HCC" "no-store"
done

echo "== 契约 6：上游不可达返回 502 JSON =="
docker rm -f "$NET-web-dead" >/dev/null 2>&1 || true
start_web_container "$NET-web-dead" "http://127.0.0.1:1" ""
DEAD_IP="$(wait_for_web_container "$NET-web-dead")"
STATUS="$(curl -s -o "$TMP_DIR/proxy-body" -w '%{http_code}' --max-time 10 "http://$DEAD_IP:8080/v1/auth/me")"
assert_eq "上游不可达时的状态码" "$STATUS" "502"
assert_eq "上游不可达时的响应体" "$(cat "$TMP_DIR/proxy-body")" '{"error":"gateway_unreachable"}'
rm -f "$TMP_DIR/proxy-body"

echo ""
echo "PASS: Web 前门（nginx）同源代理契约全部满足"

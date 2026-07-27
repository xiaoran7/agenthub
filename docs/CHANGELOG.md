# Changelog

> 所有功能变化都记录在 `Unreleased` 下，格式遵循
> [Keep a Changelog](https://keepachangelog.com/)。

## [Unreleased]

### Added

- Gateway 新增 owner 维度实时变化通道 `GET /v1/stream`（Server-Sent Events）：
  批次入库且确实产生新事件时推送 `event: sessions`，数据仅
  `{"device_id","accepted"}`，会话内容仍只能从既有读接口获得；20 秒注释心跳、
  单 owner 并发 8 条流上限（超出 429 `too_many_streams`）、重放批次不推送、
  进程关闭时统一收流。Store 的 `ingestBatch` 随之返回
  `{accepted, owner_id, changes}`，`changes` 是本批次前后活动状态确实发生变化的
  会话（内存与 PostgreSQL 两侧同一契约，PostgreSQL 在锁定设备行的事务内前后各读
  一次快照状态）。
- Gateway 新增 Web Push：`GET /v1/push/key`、`POST`/`DELETE
  /v1/push/subscriptions`，会话跃迁进入 `WAITING_INPUT`/`WAITING_PERMISSION`
  时向该 owner 的订阅推送通知，推送服务返回 404/410 的订阅立即清理。加密
  （RFC 8188 aes128gcm + RFC 8291）与 VAPID 授权（RFC 8292）基于 `node:crypto`
  自实现，用 RFC 8291 官方示例向量逐字节钉死，不引入新依赖。未配置完整 VAPID
  三元组时功能整体关闭（`push_disabled`）而不是半开；新增
  `npm run vapid-keys` 生成密钥对。订阅 endpoint 必须是无凭据的 HTTPS URL，
  p256dh/auth 按点格式与长度校验；订阅表随 0003 迁移落库，删除按 owner 隔离。
- 面板接入 Web Push：新增 `apps/web/public/sw.js`（只处理 `push` 与
  `notificationclick`，**不注册 fetch 处理器、不做离线缓存**）、设置面板"推送
  通知"开关（不支持/网关未配置/权限被拒三种状态各自禁用并说明原因）、订阅与
  注销走 `/v1/push/subscriptions`。点击通知聚焦已打开的面板并通过 postMessage
  传递会话引用后自动选中该会话，标识不进 URL；没有已打开窗口时只打开面板首页。
  订阅登记失败会撤销刚建立的浏览器订阅，避免出现网关不知道的"死订阅"。
- 面板接入实时通道：变化到达即刷新列表与详情时间线，轮询降级为兜底（连上流
  30 秒、断流 5 秒），顶部状态显示「实时 / 轮询」。断流按 1→15 秒指数退避重连，
  流不可用时行为回落到原来的 5 秒轮询。
- `apps/web` 新增 `app/v1/stream/route.ts` 流式代理：Next 的 `/v1/:path*`
  rewrite 会缓冲响应体，SSE 经它转发永远到不了浏览器，改由 Route Handler 直接
  透传上游 body，同源代理与「网关不开 CORS」的结构保持不变。

- M3 响应式 Web 面板（`apps/web`，Next.js 16 + React 19，纳入 npm workspace
  与根质量门）：设备列表（在线状态、最近上报时间、点击筛选）、会话五类视图
  （全部/活动/等待处理/最近完成/历史，视图是可重叠的过滤器而非分区）、会话
  详情与状态时间线、5 秒轮询（页面不可见时暂停周期轮询但首次拉取总是执行）、
  移动端全屏详情、深浅色主题与 PWA manifest。开发期经 Next rewrites 把 `/v1`
  代理到网关（默认 `http://127.0.0.1:3001`，可用 `GATEWAY_ORIGIN` 覆盖），
  浏览器只访问同源地址，网关无需开 CORS。网关地址与 `x-user-id` 存
  localStorage，不进 URL；直连地址校验沿用 Runner 出站策略（HTTPS 或回环
  HTTP，禁凭据/查询/片段）。
- Gateway 新增 `GET /v1/sessions/events`（Session 状态时间线）：按
  `device_id + agent_type + agent_session_id` 返回该会话的历史观察事件，
  sequence 降序，`limit` 默认 100、上限 500；查询按 owner 隔离，未知参数与
  非法 limit 拒绝。内存与 PostgreSQL 双 store 实现同一契约，PostgreSQL 增加
  0002 迁移建立 `(device_id, agent_type, agent_session_id, sequence DESC)`
  表达式索引。内存 store 的事件表随之保存 `received_at`，与 PostgreSQL 返回
  形状一致。

- 设备注册增加 Ed25519 私钥持有证明：Runner 对
  `{binding_code, device_id, public_key}` 紧凑 JSON 签名，Gateway 验签后才接受
  注册，公开可见的公钥不能再被第三方抢注绑定到其他账号。
- `sync push` 在没有可发送批次但 Outbox 仍有事件时输出警告，提示这些事件是在
  没有 `--identity` 的情况下采集、归属另一个设备 ID。
- 跨语言签名 Fixture 扩展出 `raw_status_detail` 多键、`waiting_reason` 与
  `started_at` 字段，锁定 detail 键排序在 Node 与 Rust 之间的字节一致性。
- 新增 `.gitattributes` 强制 `*.sql` 等文本文件使用 LF，避免迁移校验和随检出
  平台漂移。

### Changed

- PostgreSQL 集成测试补上 `ingestBatch` 新返回值与推送订阅的断言：该测试在本机
  始终 SKIP，旧断言 `assert.equal(await ingestBatch(...), 1)` 在真实数据库上第
  一次运行就会失败。
- 采集守护进程遇到本地 SQLite 暂时性错误（例如 hook 进程并发写导致的
  SQLITE_BUSY）不再退出，改为按失败退避重试并输出稳定错误码；SQLite 连接同时
  设置 5 秒 busy_timeout。
- Outbox 入队前先与本地会话快照合并，同步事件流携带与本地一致的"保留最近
  活动状态"语义（Codex `notLoaded` 不再把云端会话状态抹成 UNKNOWN）；事件仍
  保留本次观察自身的 `event_id` 与 `observed_at`。
- Claude `agents --json` 中缺少会话 ID 的单个条目改为跳过，不再让整批采集
  失败（与 Codex thread 列表行为一致）。
- Claude `blocked` 且 `waitingFor` 无法识别时映射为 `UNKNOWN`（Inferred），
  不再以权威置信度伪造 `WAITING_INPUT`。
- Codex 未识别的 thread 状态与 app-server 方法映射为 Inferred 置信度，与
  `notLoaded` 同等对待；thread 列表缺 `cwd` 时回退到已记住的项目路径，且不再
  用空值覆盖记忆。
- 状态合并在保留上一活动状态时同时保留其 `waiting_reason`；
  `last_activity_at` 合并后不会倒退。
- 移除 `SessionObservation` 中从未填充的会话级 `display_name` 遗留字段，
  代码与"任务标题不进入云端"的决策完全一致。
- TypeScript 编译目标升至 ES2024（需要 `String.prototype.isWellFormed`，
  仓库已要求 Node >= 22）。

### Fixed

- PostgreSQL 集成测试的 `listSessions` 过滤断言从 `WORKING` 修正为
  fixture 实际的 `WAITING_PERMISSION`：该测试因环境门控在本机始终 SKIP，
  错误断言从未暴露，真实数据库上第一次运行就会失败。
- Claude 采集成功路径先关闭 Windows Job Object 再等待读取线程：后代进程持有
  stdout 写端时不再造成守护进程永久挂死（超时路径此前已正确处理）。
- Unix 侧采集子进程放入独立进程组，超时与清理时对整组发送 SIGKILL，对齐
  Windows Job Object 的进程树终止语义（本机为 Windows，Unix 路径为交叉编译
  验证）。
- Codex app-server 响应匹配不再吞掉与请求 id 撞号的服务端请求：带 `method`
  的消息一律进入被动通知队列，审批/输入信号不会因 id 冲突丢失。
- Codex 协议行读取改为边读边限长，无换行的异常输出不再在 16MiB 上限检查前
  无界占用内存。
- 亚秒轮询间隔不再被取整为零导致忙循环（调度使用完整 Duration 精度）。
- 网关侧内存 store 的事件重复检测改用深度相等（与 PostgreSQL 路径一致），
  不再依赖 JSON 键序。
- 注册失败（如 device_conflict）时释放一次性绑定码，不再烧毁后阻止换设备
  重试。
- PostgreSQL 迁移校验和在计算前归一化 CRLF，Windows 检出与 Linux 部署不再
  因行尾差异触发启动失败。

- 初始化 Agent-Platform 独立 Git 仓库及 Solum 风格的 Agent 工作规范、变更日志、
  踩坑集和杂项决策记录。
- 建立 Rust Runner：Claude/Codex Session 状态标准化、Hook 事件适配、工作目录
  HMAC 脱敏、SQLite Outbox、Session 快照和 Fixture 隐私契约测试。
- 增加 `collect-once` 命令，可调用 Claude `agents --json` 或通过 Codex
  app-server `thread/list` 采集一次 Session 元数据。
- 增加 `run` 常驻采集命令：Claude/Codex Adapter 可独立或同时运行，根据活动状态
  使用 2/10 秒自适应轮询，失败时指数退避到最多 60 秒，并支持 `Ctrl-C` 协作式退出。
- 增加 SQLite Adapter 健康状态和 `outbox health` 诊断命令；失败只记录固定安全
  错误码，不保存可执行路径或原始系统错误。
- 为一次性与持续采集增加可配置的强制超时；Windows 使用 Job Object 在超时或退出时
  终止完整采集进程树。
- Codex app-server 改为后台读取协议消息，被动合并状态、Turn、审批和用户输入通知，
  同时保留 `thread/list` 与 `thread/loaded/list` 轮询兜底。
- 增加 Ed25519 本地设备身份：设备 ID 从公钥派生，身份可创建、加载、校验和展示，
  并可通过全局 `--identity` 同时提供设备 ID 与项目指纹密钥。
- 增加版本化同步批次协议及 `sync prepare`、`sync verify`、`sync ack` 诊断命令；
  批次包含固定白名单事件、单调序列、设备公钥和签名。
- 建立 npm workspace、共享 TypeScript 协议包和 Fastify Gateway API；支持开发期
  一次性设备绑定、签名批次验收、内存幂等写入、设备列表和 Session 过滤查询。
- 增加 Node 生成、Rust 验证的协议版本 1 跨语言签名 Fixture，并将 Rust、Node、
  TypeScript 与 Prettier 检查纳入仓库质量门。
- 增加 Runner `device bind` 和 `sync push`：可直接登记设备公钥、上传有界签名
  批次、校验 Gateway 确认并清理已确认 Outbox 事件。
- 增加可配置全局超时、稳定网络错误分类和 Gateway URL 安全校验；支持 HTTPS 远程
  地址，并为本地开发保留回环 HTTP。
- 增加 PostgreSQL Gateway Store、版本化 SQL 迁移和 `migrate` 命令；设备登记、
  事件幂等写入、序列水位、Session 快照与查询可跨服务重启持久化。
- 增加环境门控的 PostgreSQL 集成测试，在随机测试 Schema 中验证迁移幂等、批次
  重放、用户隔离和重连后的持久化查询。

### Fixed

- Outbox 序列号在已确认事件删除后仍保持单调递增，并兼容已有 Outbox 事件的
  序列水位回填。
- Codex `active + waitingOnApproval` 不再误报为“正在工作”，统一映射为
  `WAITING_PERMISSION`。
- Outbox 诊断命令不再错误要求项目指纹密钥。
- 持续轮询检测到 Session 快照没有变化时不再重复生成 Outbox 事件，避免空闲状态下
  本地队列持续增长。
- Claude CLI 或 Codex app-server 卡住时不再无限阻塞采集线程；协议读取线程和管道会
  在子进程树终止后可靠回收。

### Security

- 根 package.json 增加 npm `overrides`，把 Next.js 依赖链中的 postcss 顶至
  8.5.23、sharp 顶至 0.35.3，消除三个高危 advisory（postcss sourceMappingURL
  路径穿越与 XSS、sharp 捆绑 libvips CVE）；受影响范围覆盖 Next 全部现行
  版本，无法靠升级 Next 本体解决，`npm audit` 现为零漏洞。
- Claude `agents --json` 路径的 `kind`、`waiting_for`、`cli_status` 值改为
  经过共享消毒器（64 字符 + 固定字符集）再入 detail，`raw_status` 钳制到协议
  上限 128 字符；此前这些 CLI 控制的字符串会原样进入同步字段，未来 CLI 若在
  `waitingFor` 内嵌任务文本将原样上云。Codex 方法名与 source 同样走共享
  消毒器。
- 会话工作目录恰为当前用户主目录时显示名固定为 `home`，不再把账号用户名当作
  项目名上传。
- Rust 同步结构启用 `deny_unknown_fields`：`sync verify` 不再对注入了未签名
  附加字段的批次文件报告有效，签名语义从"已知字段投影"收紧为"结构完全匹配"。
- 协议 detail 键排序明确为 Unicode 码点升序并限制键为 snake_case ASCII：
  Node 侧此前使用 locale 相关的 `localeCompare`，混合大小写或非 ASCII 键会
  与 Rust BTreeMap 字节序算出不同签名字节，导致合法批次被拒收、Outbox 卡死
  （当前所有键均为小写 ASCII，属潜伏态；已同步收紧空 detail 对象为非法）。
- 合约层 UUID 只接受小写：PostgreSQL 将 uuid 归一化为小写，大写变体会让丢失
  ack 后的合法重放批次误判 `sequence_conflict` 而永久卡死。
- 合约层拒绝孤立代理项（lone surrogate）字符串：此前能穿过白名单，在内存
  store 正常入库、在 PostgreSQL jsonb 报 500，两个 store 行为分叉且数据在
  Rust 侧无法解析。
- Gateway 验签对非法公钥的 `createPublicKey` 异常做防御性捕获，统一返回
  401 invalid_signature 而不是 500。
- Runner HTTP 客户端禁用重定向跟随：3xx 不再可能把请求重发到 HTTPS/回环
  策略未校验过的地址。
- 设备身份文件改为同目录临时文件 + 不覆盖式落位的原子写入，写入中途崩溃不再
  留下无法自愈的半写身份文件；Unix 侧补充目录 fsync。
- 标准化层只输出固定元数据白名单；测试夹具中的 Prompt、命令、预览、完整路径和
  敏感标记不得进入事件或 Outbox。
- Codex 初始化显式退出消息、推理、计划、Diff、命令输出等内容型通知，被动事件合并
  只允许标准化后的 Session 元数据进入 Outbox。
- 设备身份文件使用“仅新建”语义，禁止静默覆盖；Unix 创建权限为 `0600`。同步批次
  不包含私钥，篡改事件或批次元数据会导致签名校验失败。
- 同步确认必须匹配协议版本、设备和批次，且不得越过实际发送的序列范围，避免误删
  尚未发送的 Outbox 事件。
- Gateway 对每一层同步对象执行字段白名单，拒绝未知字段、未知 Schema、控制字符、
  非法时间、重复事件 ID、序列间隙、篡改签名和未注册设备。
- Claude Agent 名称和 Codex Thread 标题不再进入标准化事件；同步线协议移除
  Session `display_name`，防止默认上传可能来源于用户任务文本的标题。
- Runner 拒绝向非回环明文 HTTP 地址同步，也拒绝包含凭据、查询或片段的 Gateway
  URL；网络错误与服务端错误正文不会写入本地状态或用于清理 Outbox。
- PostgreSQL 批次接收以设备行锁和单事务保护，任何事件冲突、序列间隙或写入失败均
  整批回滚；迁移记录 SHA-256 并拒绝同版本 SQL 漂移。

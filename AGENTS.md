# AgentHub（智能体集成中心）工作规范

> 原生 Remote 迁移规则（2026-09-08）：新链路以 docs/PRD.md、docs/SYNC_PROTOCOL.md 为准，允许对显式接入的原生会话发送输入、审批和中断，并向授权 Gateway 同步内容；旧只读红线仍约束发现采集。新 Host 不读取、缓存或刷新 Provider 凭据，运行状态必须来自原生协议。此规则优先于旧阶段限制。

> 适用于在本仓库工作的 Claude Code、Codex、其他智能体及人类协作者。

## 一、开工前

1. 阅读 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)，确认任务符合当前架构。
   如果需求、代码和架构不一致，先在文档中说明取舍，再修改实现。
2. 按关键词检查 [docs/PITFALLS.md](docs/PITFALLS.md)，避免重复踩坑。
3. 执行 `git status --short`，确认现有工作区改动的归属。
4. Rust 改动前运行一次 `cargo test --workspace`，记住测试基数。
5. TypeScript 改动前运行 `npm.cmd test` 和 `npm.cmd run typecheck`；Windows
   PowerShell 必须调用 `npm.cmd`，不要调用会被执行策略拦截的 `npm.ps1`。

## 二、工作红线

- **只读优先**：第一阶段不得向 Agent 发送消息、批准权限、停止任务或恢复
  Session。
- **凭据隔离**：默认不得读取 Claude/OpenAI Token、Cookie、登录文件或 OAuth 状态；
  用户明确启用凭据管理扩展后，原始凭据仍只能留在 Runner 的本地加密缓存中，绝不复制
  或上传到 Gateway、Outbox、日志或浏览器。
- **元数据白名单**：不得默认保存或同步 Prompt、Assistant 消息、代码、Diff、
  Shell 命令、工具输入输出、完整工作目录、环境变量、`.env`、transcript 或 rollout。
- **工作目录脱敏**：只保留项目目录显示名和设备密钥派生的 HMAC 指纹。
- **状态双维模型**：`Presence` 和 `ActivityStatus` 必须独立；“历史会话”是
  `Presence=PERSISTED`，不是活动状态。
- **不伪造确定性**：无法由官方接口或 Hook 可靠判断的状态必须标为 `UNKNOWN`，
  并保留来源和置信度。
- **主动出站**：Runner 不要求用户开放公网端口；Hook 不直接连接云端。
- **真实 CLI 后验**：优先使用 Fixture、单元测试和协议模拟验证；完成实现后再做
  本机真实 Claude/Codex CLI 验证，避免把开发流程耦合到本机安装状态。

## 三、完工后

1. 每次任务至少更新以下一份留痕文档：
   - [docs/CHANGELOG.md](docs/CHANGELOG.md)：新增、修改、修复或安全行为变化。
   - [docs/PITFALLS.md](docs/PITFALLS.md)：调试耗时、失败尝试、环境坑和规避方式。
   - [docs/MISC.md](docs/MISC.md)：否决方案、设计取舍、非功能性观察。
2. 用户可见命令、配置或目录发生变化时同步 README；架构发生变化时同步
   `docs/ARCHITECTURE.md`。
3. Rust 质量门全部通过后才算完成：

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   ```

   TypeScript 变更同时要求：

   ```bash
   npm.cmd test
   npm.cmd run typecheck
   npm.cmd run format:check
   ```

4. 不在仓库中留下临时数据库、日志、真实 Session 样本或一次性脚本。
5. 任务完成且质量门通过后自动提交，不必再次询问。一个独立功能或文档批次一个
   commit，提交信息清楚说明行为变化。
6. 自动提交不包含 `push`；只有用户明确要求时才推送。
7. 提交前检查 `git status`，只暂存本次任务文件，不使用会误收其他改动的
   `git add -A`。
8. 如实汇报未验证项和环境阻塞，不能把 Fixture 通过写成真实 CLI 已通过。
9. **打包发版铁律**：凡执行 Android APK、Tauri 桌面包或发版编译时，必须同步递增 `versionCode` 与 `versionName`（以及 `package.json` / `tauri.conf.json` / `Cargo.toml` 版本），产物带显式版本后缀（如 `AgentHub-vX.Y.Z.apk` / `.exe`），严禁覆盖旧版本。

## 凭据管理扩展（用户明确授权后适用）

本项目现支持可选的本地凭据管理功能：用户明确启用后，Runner 可以按 provider allow-list
读取 Claude/Codex 的 auth 文件、`.credentials.json`、OS Keychain/Keyring 和授权环境变量，
用于本机加密缓存、官方认证刷新、配额快照和重新认证。原始 access/refresh token 不得进入
Outbox、SyncBatch、日志、错误消息、浏览器或默认 Gateway API；只有显式的本地 reveal 命令
可以输出原始 token。不得递归扫描用户目录、读取浏览器 Cookie 或上传完整凭据文件。

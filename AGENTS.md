# 项目约束

> 原生 Remote 迁移规则（2026-09-08）：新链路以 docs/PRD.md、docs/SYNC_PROTOCOL.md 为准，允许对显式接入的原生会话发送输入、审批和中断，并向授权 Gateway 同步内容；旧只读红线仍约束发现采集。新 Host 不读取、缓存或刷新 Provider 凭据，运行状态必须来自原生协议。此规则优先于旧阶段限制。

通用约束继承 [AGENT.md](../../../AGENT.md)。

- 先读任务相关的 [架构](docs/ARCHITECTURE.md)、[同步协议](docs/SYNC_PROTOCOL.md) 与 [坑点](docs/PITFALLS.md)。需求与现行边界冲突时先明确取舍，不能把规划功能当已实现。
- 既有只读采集默认只同步元数据白名单，不包含 Prompt、正文、代码、diff、shell、工具载荷、完整目录、环境变量或 transcript；目录只保存显示名与设备 HMAC 指纹。
- `Presence` 与 `ActivityStatus` 独立；历史会话是 `PERSISTED`，不确定状态标 `UNKNOWN` 并保留来源、置信度。
- Runner 主动出站，Hook 不直连云端；真实 CLI 验证在 Fixture/单元/协议验证之后。
- 新 Remote 链路不读取、缓存或刷新 Provider 凭据；登录由原生 Agent 管理。旧凭据模块仅属迁移兼容，不接入新 Host，不递归扫描用户目录或浏览器 Cookie。
- 产品目标以 2026-09-07 PRD 为准：跨端接续同一原生会话；discover/resume/attach 严格区分。历史发现不代表可控制，只有原生协议确认才报告启动、审批或中断结果。
- 已接入 Remote 的会话允许向授权 Gateway 同步正文与工具事件；旧只读扫描不自动扩大范围，历史全文须显式导入。Provider raw 与 Hub Envelope 分开，内容不进入诊断日志。
- Rust 变更通过 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`；TS 变更通过 `npm.cmd test`、`npm.cmd run typecheck`、`npm.cmd run format:check`。
- `docs/CHANGELOG.md` 记录变更，PITFALLS/MISC 仅写新增可复用结论。文档批次独立提交，不要求运行无关语言测试。
- 打包同步 Android、Node/Tauri、Cargo 与 UI 版本。

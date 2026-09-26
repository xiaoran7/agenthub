# 项目规则

## 工作约定

- 只在本项目内维护规则、记忆和产物；跨仓操作须有任务依据。先检查 Git 状态，保留其他会话改动；提交、推送、部署分别按用户授权执行。
- 按当前代码、清单和配置核对文档；测试、设备验收和生产状态分别报告，历史记录不能证明当前可用。
- 不提交凭据、私钥、Cookie、真实个人资料或运行数据库。删改前核对路径、归属和用途，不动他人改动、源素材、签名材料及私人日志。
- 发版或部署必须递增实际产品版本并同步相关清单；Android/Harmony 同步 versionCode/versionName。纯文档整理不虚构发版。
- README 放启动与导航；本文件放约束；docs 放契约和经验；CHANGELOG 放历史；MEMORY 放偏好。同一事实只维护一处，删改同步修复引用；Markdown 用 UTF-8 无 BOM。
- 临时产物放本项目 `_scratch/`，交付物用项目既定路径或 `Deliverables/`；验证新版后清理本项目过期产物，保留必要回滚及指定基线。收尾只停止本次启动的进程。

## 项目边界

AgentHub 是**官方 Agent 会话的统一远程继续入口（Remote-only）**。Desktop、Web、Android 必须消费同一套 Session 模型；`agenthubd` 是本机会话控制面，Gateway 只中继和回放。读取会话历史只服务于“判断并执行继续”，不做工作台、归档或浏览功能。


## 产品边界

按任务阅读 [PRD](docs/PRD.md)、[架构](docs/ARCHITECTURE.md)、[远程协议](docs/SYNC_PROTOCOL.md) 与 [坑点](docs/PITFALLS.md)。

- 不自建 Agent 决策循环，不模拟 Provider 成功状态，不读取或代管 Provider 凭据；登录与推理由官方 Runtime 负责。
- 允许从 AgentHub 主动创建官方 Session，也允许恢复官方历史。`discover`、`resume`、`attach` 严格区分；发现 PID 或 transcript 不代表取得控制权。
- 历史读取保持最小面：transcript 解析只在 `agenthubd`（`crates/runner/src/conversation.rs`）单点实现，客户端经本地 IPC 消费；不得在客户端直接解析 Provider 文件，不得为暂不能远程控制的 Provider 提供全文浏览。
- Desktop 本地操作不得依赖 Gateway。网络中断不销毁 Host，Web/Android 只在本机 Host 可达且控制权明确时允许写操作。
- 同一原生 Session 默认单写者。不能为了“无缝接管”抢占官方客户端正在持有的 writer；无法验证 attach 时保持只读或显式 resume。
- Provider 差异保留在适配层和原始事件中；客户端只展示经过能力探测确认的模型、权限、命令与审批动作。
- 远程命令必须幂等且可审计。排队、Provider 接受和执行完成是不同状态；结果未知时先对齐，不能自动重放可能产生副作用的命令。

## 数据与安全

- 历史发现默认只同步元数据白名单，不上传 Prompt、正文、代码、diff、shell、工具载荷、完整路径、环境变量或 transcript。
- 进入 managed Remote 的 Session 可将正文、工具事件和审批同步到用户授权的 Gateway；原始 Provider 事件与统一 Envelope 分层保存，内容不进入诊断日志。
- Runner/agenthubd 主动出站，远端不要求用户开放本机端口。生产使用 TLS；浏览器和移动端不持有设备私钥或 Provider 凭据。
- `Presence`、`ActivityStatus`、`ControlState` 分开建模。不确定状态标 `UNKNOWN`，并保留来源和时间。

## 实施与验证

- 当前二进制仍名为 `agent-pulse-runner`；`agenthubd` 是已落地的进程职责。Desktop 与 Remote 必须持续共用同一 Host；独立改名和 OS 服务安装属于后续里程碑。
- 新链路优先完成 Codex 常驻 Host → Desktop 本地 IPC → Gateway Remote；再按官方能力接入 Claude Code，其他 Provider 先保持发现/只读。
- Rust 变更运行 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`；TS 变更运行 `npm.cmd test`、`npm.cmd run typecheck`、`npm.cmd run format:check`。
- 真实 CLI、桌面安装、Android 模拟器/真机、Gateway 重启和生产部署分别记录证据，不能互相替代。
- 行为、接口与架构决策写入 `docs/CHANGELOG.md`。文档批次可独立提交，不要求运行无关构建。
- **强制版本递增铁律**：凡进行代码修复并触发打包或生产部署，必须递增版本号（Patch/Minor）。打包时严格同步全平台：Android（自增 `versionCode` 与 `versionName`）、Node/Tauri、Cargo 与 UI 契约版本，绝对禁止使用相同版本号二次打包。

长期偏好见 [MEMORY.md](MEMORY.md)。

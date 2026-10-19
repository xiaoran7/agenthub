# 开源 SEO 与曝光实操指南 (Open Source Guide)

## 1. GitHub 仓库元数据配置

- **Description**（≤130 字符）：
  `Remote Web console & session manager for Claude Code and OpenAI Codex. Continue sessions anywhere via browser or mobile PWA.`
- **Topics**：
  `claude-code`, `openai-codex`, `ai-agent`, `agent-runner`, `session-manager`, `remote-control`, `web-ui`, `dashboard`, `self-hosted`, `pwa`, `developer-tools`, `rust`, `typescript`, `react`, `cli`
- **Home Page 勾选项**：勾选 `Releases`、`Packages`。
- **Social Preview**：上传 `1280 x 640` 预览图（主标题 `AgentHub` + 副标题 `Remote Web Console for Claude Code & Codex` + 控制台截图）。

## 2. 社区投稿与外链矩阵

- **Awesome 列表与 HelloGitHub**：按 [COMMUNITY_SUBMISSIONS.md](COMMUNITY_SUBMISSIONS.md) 规范提交 PR。
- **阮一峰《科技爱好者周刊》**（`https://github.com/ruanyf/weekly/issues`）：
  - **标题**：`开源项目推荐：AgentHub - 官方 Agent 会话的跨设备远程接管与 Web 控制台`
  - **要点**：解决终端跑 Claude Code / Codex 离开工位后卡在权限审批与手机 SSH 难用的问题；纯 Rust 单文件内嵌控制台、主动出站零公网端口、手机 PWA 随时审批与急停。
- **V2EX / Reddit / HN / X 宣发**：直接使用 [LAUNCH_COPYWRITING.md](LAUNCH_COPYWRITING.md) 对应模板。
- **GitHub Discussions 种子问答**：按 [DISCUSSIONS_QA_SEEDS.md](DISCUSSIONS_QA_SEEDS.md) 初始化 3 篇 Q&A 并勾选 `Mark as answer`。

## 3. 发布前速查清单

- [x] 根目录提供英文 `README.md`、中文 `README_CN.md` 与 `LICENSE`（MIT）
- [x] 提供大模型检索索引 `llms.txt` 与 `llms-full.txt`
- [x] 明确标注上游开源项目溯源（magpie、orca、ACP）
- [ ] GitHub 仓库已配置 Description、15 个 Topics 与 1280×640 Social Preview

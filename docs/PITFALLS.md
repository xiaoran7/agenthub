# 踩坑集

> 记录调试耗时、失败尝试和环境陷阱。每条包含现象、根因、解决方式和规避方法。

## 2026-07-25 WindowsApps 中的 Codex 可发现但不可执行

- **现象**：`Get-Command codex` 能返回
  `C:\Program Files\WindowsApps\...\codex.exe`，但执行 `codex --version`
  返回 `Access is denied`。
- **根因**：Windows App 安装目录中的资源可被命令解析器发现，不代表当前 Shell
  拥有直接执行该内部二进制的权限。
- **解决方式**：代码侧允许 `--binary` 显式指定独立安装的 Codex CLI；真实协议验证
  等可执行 CLI 安装后再运行，不能把“能找到路径”当作“CLI 可用”。
- **如何避免**：环境探测必须实际执行无副作用的 `--version`，不能只使用
  `Get-Command` 或文件存在性判断。

## 2026-07-25 PowerShell 默认编码导致中文 README 显示乱码

- **现象**：`Get-Content -Raw` 输出 UTF-8 中文 README 时显示为乱码，但文件内容并未
  损坏。
- **根因**：当前 PowerShell 的默认文本编码与无 BOM UTF-8 文件不一致。
- **解决方式**：读取仓库中文文档时显式使用 `Get-Content -Encoding UTF8`。
- **如何避免**：看到乱码先核对读取编码，不要在未确认文件实际字节前重写文件。

## 2026-07-25 `git diff --check` 拦截文件尾多余空行

- **现象**：初始提交前质量检查返回多处 `new blank line at EOF`，同时输出 Windows
  的 LF/CRLF 转换提示。
- **根因**：部分文件结尾是两个换行，形成额外空白行；CRLF 提示只是工作区换行约定，
  与真正导致检查失败的空白行不是同一个问题。
- **解决方式**：删除文件尾额外空白行后重新暂存，再执行 `git diff --cached --check`。
- **如何避免**：把 `git diff --cached --check` 固定放在 commit 前，并分别处理
  whitespace error 与行尾转换 warning。

## 2026-07-25 PowerShell 将正常 stderr 日志包装成 `NativeCommandError`

- **现象**：持续 Runner 烟雾测试把 `2>&1` 赋值给变量时，JSON 轮询日志前出现
  `cargo : ... NativeCommandError`，但进程退出码仍是 0，健康状态也正确落库。
- **根因**：Runner 按守护进程惯例把运行日志写到 stderr；Windows PowerShell 在捕获
  原生命令 stderr 时会把每行包装为 ErrorRecord，这不表示子进程执行失败。
- **解决方式**：自动化同时检查 `$LASTEXITCODE` 和 JSON 内容，不用 ErrorRecord 的
  外观替代退出码判断。
- **如何避免**：命令级测试若需要机器读取日志，优先分别捕获 stdout/stderr；以后接入
  正式日志协议时为诊断输出提供明确的 JSONL 文件或结构化日志目标。

## 2026-07-25 只杀直接子进程无法解除 Windows 管道阻塞

- **现象**：Claude 超时测试虽然在截止时间杀掉了 `.cmd`，采集函数仍等待约 5 秒，
  直到脚本启动的 PowerShell 后代自然退出。
- **根因**：后代进程继承了 stdout 管道写端；直接子进程退出后，读取线程仍看不到
  EOF，所以 `join` 会继续阻塞。
- **解决方式**：为每个采集进程创建 Windows Job Object，设置
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，把直接子进程加入 Job；超时和连接关闭时终止
  Job 中的完整进程树，再等待进程和读取线程回收。
- **如何避免**：所有带管道和超时的外部命令测试都要让脚本再派生一个持有管道的后代，
  不能只测试单进程 `sleep`；Unix 实现时也应使用进程组而不是只发送单 PID 信号。

## 2026-07-25 PowerShell 执行策略拦截 `npm.ps1`

- **现象**：Node 可正常执行，但直接运行 `npm --version` 报告 `npm.ps1 cannot be
  loaded because running scripts is disabled`。
- **根因**：PowerShell 命令解析优先选中了 Node 安装目录中的 `npm.ps1` 包装脚本，
  当前系统执行策略禁止该脚本；npm 本身和对应的 Windows 命令文件并未损坏。
- **解决方式**：仓库命令和自动化在 Windows 上显式调用 `npm.cmd`。
- **如何避免**：工具链探测要分别检查 `node --version` 与 `npm.cmd --version`；
  不要为了运行 npm 全局放宽 PowerShell 执行策略。

## 2026-07-25 本机缺少 PostgreSQL 与容器运行时

- **现象**：PostgreSQL Store 的类型检查和内存契约测试可执行，但找不到 `psql`、
  PostgreSQL Windows 服务、Docker 或 Podman，无法在本机启动真实数据库。
- **处理方式**：集成测试要求显式设置 `AGENT_PLATFORM_TEST_DATABASE_URL`，并在
  专用数据库下创建随机 Schema；没有连接串时报告 `SKIP`，不把跳过写成数据库通过。
- **如何避免**：CI 增加 PostgreSQL Service Container，并把该环境变量指向专用测试
  数据库；开发机不要为了单次验证静默安装系统服务或把测试指向生产数据库。

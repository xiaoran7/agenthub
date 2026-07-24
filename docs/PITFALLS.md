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

# MISC 杂项记录

> 记录不属于功能变更、但值得保留的方案取舍与非功能性观察。

## 2026-07-25 第一阶段 Runner 设计取舍

- **“历史”不作为活动状态。** 一个历史 Session 仍可能保留“最近一次执行失败”
  这样的活动结果；把两者压进同一个枚举会丢信息。因此使用 `Presence` 表示
  实时/持久化/未知，使用 `ActivityStatus` 表示工作、等待、完成等活动状态。
- **优先官方只读接口和 Hook，不扫描私有登录文件。** Claude 使用
  `agents --json` 与生命周期 Hook；Codex 使用 app-server 与 Hook。即使扫描本地
  transcript 能得到更多内容，也会扩大隐私面并绑定不稳定的内部格式，因此否决。
- **项目关联使用显示名加 HMAC 指纹。** 只传 basename 无法区分多块磁盘或不同父目录
  下的同名项目；上传绝对路径又会泄露用户名与目录结构。设备密钥派生的 HMAC 指纹在
  同一设备内稳定，同时不暴露原路径。
- **Codex `notLoaded` 不等于历史。** 它只表示 Thread 当前未加载到该 app-server
  进程，不能据此证明任务停止或仅剩持久化记录，所以映射为未知 Presence，并由快照
  合并逻辑保留最近可信活动状态。
- **真实 CLI 验证放在实现与 Fixture 验证之后。** Runner 的开发和回归测试不应依赖
  开发机是否安装 Claude CLI，或 WindowsApps 是否允许当前 Shell 执行 Codex。

## 2026-07-25 持续采集调度取舍

- **每个 Adapter 独立退避。** Claude CLI 不可用时不能拖慢 Codex，反过来也一样；
  因此失败次数和下次尝试时间按 Agent 分开保存，而不是整个 Runner 共用一个退避器。
- **错误诊断保存分类，不保存原文。** 进程启动错误可能包含用户名、安装路径或系统
  环境细节。云端和本地健康表只需要知道 `permission_denied`、
  `executable_not_found` 等稳定分类，原始错误不进入持久化。
- **轮询事件按快照变化去重。** `observed_at` 和随机 `event_id` 每次都会变化，不能用于
  判断 Session 是否真的变化；比较时忽略这两个传输字段，保留状态、项目、来源、
  置信度和 Agent 提供的活动时间。
- **Codex app-server 在连续轮询间复用。** 每轮重新握手成本高且会制造短命进程；
  Runner 保持一个 stdio app-server，协议失败后丢弃并在下一次退避到期时重建。
- **超时覆盖进程树和读取线程的完整生命周期。** 单纯在外层计时不能解除被后代进程
  持有的 stdout 管道；实现必须在截止时间到达时终止完整进程树，等待直接子进程退出，
  再回收读取线程。Claude 静默命令和 Codex 静默握手均有故障注入测试。

## 2026-07-25 Codex 被动通知取舍

- **不为获取通知而启动或恢复用户 Thread。** Codex 官方只保证当前 app-server
  连接启动或恢复 Thread 后的 Turn/Item 通知，但 `thread/start`、`thread/resume`
  会让监控端参与 Session 生命周期，超出第一阶段只读边界。因此 Runner 只处理连接
  自然收到的通知，并以 `thread/list`、`thread/loaded/list` 轮询兜底。
- **在协议入口退出内容型通知。** 初始化的 `optOutNotificationMethods` 覆盖消息、
  推理、计划、Diff、命令输出和完整 Item 等载荷；审批、用户输入和状态通知仍可用于
  元数据判断，进入 Outbox 前继续经过字段白名单。
- **真实覆盖率是待验证事实，不是实现假设。** 当前开发机的 Codex WindowsApps
  二进制不可执行，Fixture 已验证解析、覆盖优先级和隐私边界，但跨客户端 Thread
  是否向独立 app-server 广播全部状态通知要留到真实 CLI 验证。

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

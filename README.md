# Picket

Picket 是基于 Pi Core 的桌面 Git Agent。你选择仓库、工作树与变更范围；Picket 可以查看外部 Diff、生成提交说明、审查代码，并通过一键 C-Push 自动拆分、提交和推送。需要你决定时停下来问，未选改动和 IDE 中的未暂存草稿留在原处。

## 当前状态

**M0 与 M1 已通过阶段验证，M2 进行中：真实仓库读取、受控分支切换 / 创建、Worktree 创建与无模型精确范围本地提交已接入，外部 IDE、真实模型与远端推送尚未开放。** 默认从“无仓库、无模型”开始；桌面版可以选择本机 Git 目录，查看真实工作树、分支、变更和增删统计，在顶部切换 / 创建分支或新建 Worktree，也可以手写提交说明并在确认范围后执行仅本地提交。只有明确触发这些操作时才写入 Git。IDE 联动、跨重启草稿持久化、真实模型和推送仍未接入。

- 首发 macOS，采用 Tauri 2 / Rust + React / TypeScript + 独立 Pi Core 进程。
- 有模型时以 C-Push 为主流程；无模型也能手写说明并提交。
- 部分暂存文件默认只提交已暂存范围；`main`、`master` 写入提交或推送前必须先切开发分支。
- 推送失败只重试推送，不重复创建本地提交。

## 文档与设计

| 文档 | 用途 |
| --- | --- |
| [开发文档](docs/development.md) | 已确认范围、授权、安全规则与实现边界。 |
| [UI 规格](docs/ui-spec.md) | 页面层级、字段、交互及正式功能与演示的区别。 |
| [开发计划](docs/development-plan.md) | 分阶段任务、验收清单与开发入口。 |
| [UI 冻结原稿](docs/design/picket-mvp.tsx) | 最新设计原稿的未修改副本；仅左侧桌面窗口是产品界面。 |

## 本地运行

当前验证环境是 macOS arm64，Node 24.17.0、pnpm 10.28.1、rustup 与 Xcode Command Line Tools。Rust 版本由 `rust-toolchain.toml` 固定；桌面脚本会显式使用 `~/.cargo/bin`，不需要修改全局 shell 配置。

```bash
pnpm install --frozen-lockfile
pnpm desktop dev
```

点击“打开本机仓库…”选择 Git 工作目录；关联工作树与已占用分支可以跳转查看。刷新、切换工作树和窗口重新聚焦会重新读取状态；勾选只改变界面选择，不改 index。部分暂存只显示已暂存范围，远端领先 / 落后仅基于本地跟踪引用，不隐式 fetch。

“新建分支（带走当前改动）”在原工作树切出分支，保留暂存与未暂存内容。“新建 Worktree”会选择父目录，以分支名创建独立目录（`/` 换成 `-`），从当前提交开始；原目录的改动不移动。目标目录已存在时拒绝覆盖。脏工作树只允许切到同一提交的已有分支；不同提交须先在 Git / IDE 中处理改动，或新建分支带走改动，不自动 stash。

分支操作与本地提交前后均核对 Git。`main`、`master` 和分离 `HEAD` 禁止直接提交，须先新建或切换开发分支；部分暂存默认只提交已暂存范围，未选已暂存文件和未暂存草稿留在原处。本地提交按文件核对 Rust 签发的 `commitToken`，真实运行 `pre-commit`、`prepare-commit-msg`、`commit-msg` 与 `post-commit` 钩子（不使用 `--no-verify`）；若钩子拒绝或改写待提交 / 工作区内容则在生成提交前中止并保留草稿与选择。分支检出操作仍明确拒绝可执行 hooks 与自定义 `hooksPath`。外部文件过滤器转换、已有冲突 / merge / rebase、索引锁、稀疏 / 隐藏 index 标记和子模块当前明确拒绝写入。中断或失败不会自动重跑或回滚，需刷新核对实际状态。

macOS 原生红黄绿按钮与仓库入口共用首行，真实窗口置顶已经接入。未接入的操作会明确说明，不会伪造模型连接、保存或推送成功。草稿、选择、已打开仓库和编辑器偏好仍只在当前窗口内保留；不加载开发者本机 Pi 登录或模型资源，请勿在模型表单填写真实密钥。应用图标仍为 M0 占位。

```bash
pnpm check                # TypeScript、Node 测试、Rust 格式/静态检查及测试
pnpm ui:verify            # Chrome 浏览器交互测试；仅使用开发样例
pnpm desktop build        # 生成 macOS 应用包
pnpm package:verify --ui  # 独立目录验证真实 Git 读取、分支 / Worktree 创建、精确范围本地提交及 M0/M1 回归；会打开并关闭窗口
```

应用包：`src-tauri/target/release/bundle/macos/Picket.app`。当前未做 Developer ID 签名、公证或其他 macOS 版本的发布验证。

具体证据见 [M0 记录](docs/validation/m0.md)、[M1 记录](docs/validation/m1.md)、[标题栏补充](docs/validation/m1-titlebar.md)、[M2 只读接入记录](docs/validation/m2-read.md)、[M2 分支 / Worktree 记录](docs/validation/m2-branches.md) 和 [M2 本地提交记录](docs/validation/m2-commit.md)。已在 macOS 原生目录面板选中临时 Git 仓库，并在工作台看到对应分支和文件。浏览器预览使用样例数据。

下一步继续 **M2 的外部 IDE 联动与按工作树草稿 / 选择持久化**。完整 M2 阶段验收尚未通过。

### 独立开发样例

运行 `pnpm dev` 后访问 `http://127.0.0.1:1420/dev.html?scenario=question`，可按 [M1 记录](docs/validation/m1.md) 切换执行中、提问、续写、完成及异常样例；`scenario=foundation` 保留原诊断页。该入口和样例不参与正式构建，也不表示真实 Git / 模型任务已成功。普通 `index.html` 始终从真实空态开始。

`pnpm ui:verify` 需要本机 Chrome，可用 `CHROME_PATH` 指定可执行文件；脚本使用临时浏览器目录，不复用个人登录，也不自动安装浏览器。

## Agent 与 Serena

编码助手先阅读 [AGENTS.md](AGENTS.md)。Serena 项目配置为 `.serena/project.yml`，支持 TypeScript / TSX / JavaScript 与 Rust。MCP 按客户端分别配置，通过当前工作目录定位项目，不固定开发机路径：

| 客户端 | 项目级配置 | Serena 上下文 |
| --- | --- | --- |
| Pi MCP Adapter | `.pi/mcp-adapter.json` | `agent`；保留按需启动后常驻及直接暴露工具的设置。 |
| Codex | `.codex/config.toml` | `codex`；使用 Codex 内置文件 / shell 工具，Serena 提供符号能力。 |

根目录不再定义 Serena 服务。两个客户端同时使用时会各自启动一个进程，不代表同一客户端重复加载配置；全局 MCP 设置未改。

本机需安装 Serena CLI。Rust 语言服务组件由 `rust-toolchain.toml` 声明；Serena 管理其 TypeScript 语言服务，受信任后使用项目自己的 TypeScript 编译器。Codex 的 MCP 环境值不做变量插值，因此该入口通过固定 `/bin/sh` 命令在启动时补充用户工具 PATH，不修改 shell 配置。

```bash
# 在项目根目录重建索引与检查语言服务，不修改 shell 配置
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project index .
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project health-check .
```

项目迁移后从新目录重新打开客户端会话；首次连接按客户端提示批准项目 MCP 服务。Codex 还需在客户端信任本项目，才会加载项目级配置；本仓库没有替用户修改全局信任设置。Serena 缓存、日志、memories 和机器专用配置保持本地，不进入应用包。初始化结果与最新入口迁移的验证状态见 [Serena 验证记录](docs/validation/serena.md)。

# Picket

Picket 是基于 Pi Core 的桌面 Git Agent。你选择仓库、工作树与变更范围；Picket 可以查看外部 Diff、生成提交说明、审查代码，并通过一键 C-Push 自动拆分、提交和推送。需要你决定时停下来问，未选改动和 IDE 中的未暂存草稿留在原处。

## 当前状态

**M0–M6 功能开发、应用打包加固与 MVP 验收已完成：真实仓库读取、IDEA 风格本地 / 远程双折叠分支菜单与远程分支跟踪检出、变更一键全选、无模型精确范围本地提交与提交并推送、显式拉取并变基、五类外部 IDE 本机已安装动态探测与受控联动、跨进程冷重启草稿 / 模型 / 会话记录持久化、macOS 系统钥匙串凭据存储、提交说明生成、结构化代码审查，以及一键 C-Push 自动拆分提交、远端推送、推送失败仅重试推送、冲突受控合并与切回自动对账均已完成并通过验证。** 首次打开从“无仓库、无模型”开始，之后会自动恢复上次打开的仓库、本机可用首选编辑器、已保存模型、C-Push 会话记录以及各工作树的草稿与文件勾选。桌面版可选择本机 Git 目录，点击文件在已安装的首选 IDE（IntelliJ IDEA、RustRover、WebStorm、VS Code、Cursor）打开真实文件、Diff、已暂存 Diff、行号定位或冲突合并，在顶部单分支下拉中切换本地分支、检出远程分支或切换关联工作树，手写或通过已配置模型生成提交说明、执行代码审查，使用 C-Push 一键拆分提交并推送，或在确认范围后执行“仅本地提交” / “提交并推送”。只有明确触发分支切换 / 检出、同步或提交推送操作时才执行受控 Git 写入。

- 首发 macOS，采用 Tauri 2 / Rust + React / TypeScript + 独立 Pi Core 进程。
- 有模型时以 C-Push 为主流程；无模型也能手写说明并提交。
- 部分暂存文件默认只提交已暂存范围；`main`、`master` 显示轻量保护分支标签并允许直接提交，分离 `HEAD` 提交前须先切分支。
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

顶部第二行左侧仅显示当前分支入口，点击后展开类似 IntelliJ IDEA 的“本地分支”与“远程分支”双折叠菜单（多工作树仓库额外展示“工作树”折叠组）：本地分支中已在其他工作树检出的条目可直接跳转对应工作树；点击远程分支时，本地已有同名分支则直接切换或跳转，尚无同名分支则自动基于该远程引用检出本地跟踪分支。手动命名新建分支或新建 Worktree 在 IDE / 终端执行，切回 Picket 窗口后自动刷新识别。脏工作树只允许切到同一提交的分支；不同提交须先在 Git / IDE 中处理改动，不自动 stash。

分支切换、本地提交与远端同步推送前后均核对 Git。分离 `HEAD` 禁止直接提交，须先切换到已有本地分支或在 IDE / 终端切好分支（`main`、`master` 展示轻量 `保护分支` 标签并允许直接提交与推送；禁止自动强推）；部分暂存默认只提交已暂存范围，未选已暂存文件和未暂存草稿留在原处。显式“拉取并变基”与 C-Push 内同步共用无损脏工作树备份变基路径（非重叠改动自动备份并恢复精确暂存 / 未暂存边界，重叠改动或已发布历史重写风险先暂停）。本地提交成功但远端推送失败时进入“已在本地提交，推送失败”，点击“重试推送”只推送已有提交；同步遇到冲突时可选择用户授权的 Agent 合并（支持自定义合并要求并严格校验冲突标记与文件边界）或去外部 IDE 手动合并，切回窗口后自动核对冲突与远端状态。中断或崩溃重启后先核对真实 Git，恢复写入须由用户确认，绝不自动重跑。

macOS 原生红黄绿按钮与仓库入口共用首行，真实窗口置顶已经接入。首选编辑器、已打开仓库、模型元数据、C-Push 会话记录与按工作树的提交说明草稿、勾选、Tab 会在本机应用数据目录中跨重启持久化；模型 API Key 通过 `Security.framework` 存入 macOS 系统钥匙串，普通配置文件、前端状态与错误提示均不含明文密钥，也不加载开发者本机环境变量或外部 Pi 登录。

```bash
pnpm check                # TypeScript、Node 测试、Rust 格式/静态检查及测试
pnpm ui:verify            # Chrome 浏览器交互测试；仅使用开发样例
pnpm desktop build        # 生成 macOS 应用包
pnpm package:verify --ui  # 独立目录验证真实 Git 读取、IDE 快照导出、分支 / 工作树切换、精确范围本地提交、跨重启恢复、钥匙串与 Pi 生成/审查、C-Push 拆分推送与冲突合并及 M0/M1 回归；会打开并关闭窗口
```

应用包：`src-tauri/target/release/bundle/macos/Picket.app`。当前未做 Developer ID 签名、公证或其他 macOS 版本的发布验证。

具体证据见 [M0 记录](docs/validation/m0.md)、[M1 记录](docs/validation/m1.md)、[标题栏补充](docs/validation/m1-titlebar.md)、[M2 只读接入记录](docs/validation/m2-read.md)、[M2 分支 / Worktree 记录](docs/validation/m2-branches.md)、[M2 本地提交记录](docs/validation/m2-commit.md)、[M2 IDE 联动与持久化记录](docs/validation/m2-ide-persistence.md)、[M3 模型、钥匙串与生成 / 审查记录](docs/validation/m3.md)、[M4 C-Push 自动拆分与会话执行记录](docs/validation/m4.md)、[M5 远端同步、冲突与恢复记录](docs/validation/m5.md) 和 [M6 联调、打包与 MVP 发布验收记录](docs/validation/m6.md)。已在 macOS 原生目录面板选中临时 Git 仓库，并在工作台看到对应分支和文件。浏览器预览使用样例数据。

### 独立开发样例

运行 `pnpm dev` 后访问 `http://127.0.0.1:1420/dev.html?scenario=question`，可按 [M1 记录](docs/validation/m1.md) 切换执行中、提问、续写、完成及异常样例；`scenario=foundation` 保留原诊断页。该入口和样例不参与正式构建，也不表示真实 Git / 模型任务已成功。普通 `index.html` 始终从真实空态开始。

`pnpm ui:verify` 需要本机 Chrome，可用 `CHROME_PATH` 指定可执行文件；脚本使用临时浏览器目录，不复用个人登录，也不自动安装浏览器。

## Agent 与 Serena

编码助手先阅读 [AGENTS.md](AGENTS.md)。Serena 项目配置为 `.serena/project.yml`，支持 TypeScript / TSX / JavaScript 与 Rust。MCP 按客户端分别配置，通过当前工作目录定位项目，不固定开发机路径：

| 客户端 | 项目级配置 | Serena 上下文 |
| --- | --- | --- |
| Pi MCP Adapter | `.pi/mcp-adapter.json` | `agent`；保留按需启动后常驻及直接暴露工具的设置。 |
| Codex | `.codex/config.toml` | `codex`；使用 Codex 内置文件 / shell 工具，Serena 提供符号能力。 |
| Antigravity | `.agents/plugins/picket-serena/` | `antigravity`；通过工作区插件 `mcp_config.json` 暴露 `picket-serena` 服务。 |

根目录不再定义 Serena 服务。多个客户端同时使用时会各自启动一个进程，不代表同一客户端重复加载配置；全局 MCP 设置未改。

本机需安装 Serena CLI。Rust 语言服务组件由 `rust-toolchain.toml` 声明；Serena 管理其 TypeScript 语言服务，受信任后使用项目自己的 TypeScript 编译器。Codex 与 Antigravity 插件的 MCP 环境值不做系统变量插值，因此这两个入口通过固定 `/bin/sh` 命令在启动时补充用户工具 PATH（Antigravity 插件同时从插件目录回退定位仓库根目录），不修改 shell 配置，也不写死本机路径。

```bash
# 在项目根目录重建索引与检查语言服务，不修改 shell 配置
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project index .
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH" serena project health-check .
```

项目迁移后从新目录重新打开客户端会话；首次连接按客户端提示批准项目 MCP 服务。Codex 还需在客户端信任本项目，才会加载项目级配置；本仓库没有替用户修改全局信任设置。Serena 缓存、日志、memories 和机器专用配置保持本地，不进入应用包。初始化结果与最新入口迁移的验证状态见 [Serena 验证记录](docs/validation/serena.md)。

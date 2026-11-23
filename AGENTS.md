# Picket 开发约定

本文件适用于整个仓库，指导开发 Picket 的编码助手；它不是 Picket 内置 Agent 的系统提示词。更深目录中的同名文件只约束其子目录。

## 开始工作

- 使用中文与用户沟通。说明实际完成、验证结果和未验证项，不把计划、配置存在或模拟结果说成已实现。
- 先读 `README.md` 和相关模块；阶段与验收以 `docs/development-plan.md` 为准。当前 M0 / M1 已完成，M2 进行中：仓库读取、受控分支切换 / 创建、Worktree 创建和无模型精确范围本地提交已实现，下一步为外部 IDE 联动与草稿 / 选择持久化；后续进度变化时同步文档。
- 产品行为见 `docs/development.md`，页面关系见 `docs/ui-spec.md`，视觉定稿见 `docs/design/picket-mvp.tsx`。原稿右侧演练控制台、模拟修复等不是正式功能，不能直接将整份演示代码当生产界面。
- `docs/` 按当前 `.gitignore` 仅在本地维护；缺失时说明缺少依据并询问用户，不自行补造定稿，也不擅自取消忽略。
- 先检查 `git status`，保留用户已有修改。提交、推送、重置、清理和分支操作需有用户的明确授权；产品内的 C-Push 授权规则不等于授权编码助手操作本仓库历史。

## 技术与职责

- 首发 macOS；当前实际验证的是 arm64。Node / pnpm 版本看 `package.json`，Rust 看 `rust-toolchain.toml`，具体依赖以两份锁文件为准。
- `src/`：React + TypeScript 界面，按 feature 组织；`src/desktop/` 封装类型化桌面命令与事件。
- `shared/`：界面和 Node 进程共享的协议与身份字段；Rust 对应类型必须同步，协议变更同时验证两端。
- `agent/`：独立 Pi Core 进程，不在 WebView 中运行模型或保存密钥。
- `src-tauri/`：Rust 桌面层，负责 Git 读取、受控写入协调、授权检查、系统能力、进程生命周期与恢复。`src-tauri/src/repository/` 当前开放已选择仓库的读取、使用 `branchToken` 的分支 / Worktree 操作，以及使用每文件 `commitToken` 的精确范围本地提交；远端推送仍关闭（M5）。`branchToken` 不授权提交，也不能把 UI 范围版本当作写入授权。
- `scripts/`：开发、打包与验证入口，不在前端组件中拼接 shell 命令。
- `src/features/foundation/` 与 `src-tauri/src/foundation/` 是 M0 验证实现。候选 Git 执行器仅用于临时仓库，可执行 hooks 当前明确拒绝；未通过生产验收前不得直接开放给用户仓库。
- 代码按职责拆分，测试跟随所属模块；不为了未来假设添加共享层或提前铺空目录。不直接修改依赖、生成文件、缓存或构建产物。

## 不可破坏的行为

- 有模型时 C-Push 是一键处理授权范围、拆分、提交与推送；只在需要决定时询问，不自行添加每次必经的确认页。无模型仍可手写并明确触发 Git 操作。
- 勾选只表达选择，不改 Git 暂存区。部分暂存默认只提交已暂存部分，未选已暂存文件不能混入，未暂存草稿必须保全。
- main / master 提交或推送前必须先切开发分支；不自动强推、丢弃用户改动或改写已发布历史。
- Rust 是受控 Git 写入入口，Git 是实际状态的来源。UI 与模型不能直接宣布提交成功，不能用 `git add -A` 代替范围计算。
- 异步任务绑定 repository、worktree、session、request 和范围版本；切换界面、重试、取消与晚响应不能串目标、扩大权限、覆盖较新草稿或重复提交。
- 推送失败保留已有提交，重试只推送。中断后的写入恢复先核对 Git，不能从头盲目重跑。
- 用户写说明时，新消息不得抢焦点、自动展开覆盖输入或改写文字。消息展开只覆盖下半工作区。
- 只有用户授权当前冲突合并后，Agent 才可修改相应冲突文件；审查意见不等于允许自动修复常规业务代码。
- 凭据存系统钥匙串；协议、日志、普通配置与测试样例不落真实密钥。不要复用开发者本机 Pi 登录或自动加载其资源到产品 Agent。

## 开发与验证命令

在项目根目录执行，使用 pnpm，不生成其他包管理器锁文件：

```bash
pnpm install --frozen-lockfile
pnpm desktop dev          # 桌面开发；pnpm dev 仅为浏览器预览
pnpm desktop fmt          # Rust 格式化
pnpm check                # TS 类型、Node 测试、Rust 格式/静态检查与测试
pnpm ui:verify            # Chrome 交互测试；独立开发样例，不运行真实 Git / 模型
pnpm desktop build        # 生成 macOS 应用包
pnpm package:verify --ui   # 验证最新应用包、真实临时 Git 读取、分支 / Worktree 创建与精确范围本地提交；需桌面会话，会打开并关闭窗口
```

- `scripts/desktop.mjs` 显式补充 `~/.cargo/bin`，不要为运行检查擅改用户 shell 配置。
- Git 测试使用临时仓库 / 本地 bare 远端，不能拿当前项目或用户业务仓库验证破坏性操作。
- 改界面、状态、Rust 或协议后运行相关测试与 `pnpm check`；改打包、进程生命周期、运行时或依赖时，还需重新打包并验证应用包。无法执行的检查必须明确列出。
- Node 测试和桌面检查都会生成同一套 sidecar 文件；不要将带 `sidecar:prepare` 的命令并行执行。
- 测试夹具、脚本化 provider 和真实在线服务分开报告；单个命令返回成功不代表完整 MVP 验收通过。历史验证见 `docs/validation/`。
- 保留 M0 的范围保全、重复操作、取消时序和退出清理回归；不要通过降低检查、跳过 hook 或改写验收来掩盖失败。

## Serena 与仓库维护

- 项目配置为 `.serena/project.yml`，支持 TypeScript / TSX / JavaScript 与 Rust。Pi 的 MCP 入口为 `.pi/mcp-adapter.json`，Codex 为 `.codex/config.toml`；根目录不再定义 Serena 服务。以当前活动项目的真实路径为准，不硬编码开发机用户名或旧工作目录。
- Pi 使用 Serena 的 `agent` 上下文，Codex 使用 `codex` 上下文。客户端各自启动一个 Serena 进程是正常行为；不要另加根目录重复入口，也不要把 Pi 专用的生命周期字段复制到 Codex 配置。
- 可用时优先用 Serena 定位符号与引用，再读取所需实现；工具不可用时使用 `rg` / 文件读取，不声称仅因存在配置就已连接。
- Serena 尊重 `.gitignore`。`docs/` 和设计原稿不在代码索引中，必要时直接用文件工具阅读；不要把索引无结果当作文件不存在。
- 缓存、日志、memories 和 `project.local.yml` 保持本地；可共享配置与本文件可以纳入后续提交，但本任务本身不授权提交。
- 修改 Serena 后验证 TS 与 Rust 两端的符号 / 引用；索引、健康检查需要 `~/.cargo/bin` 在当前进程 PATH 中。首次运行可下载必要语言服务，但不要改全局偏好或扩大到其他项目。
- 完成后给出变更、检查结果和剩余限制。需要改变定稿、权限、平台或阶段范围时，先向用户确认。

# 安装、迁移与升级

EdgeHome 当前以 **Git 固定提交 + Cargo.lock + 固定 Rust 工具链**交付。
新应用默认不依赖原 SDK 的磁盘路径；只需携带应用代码和版本文件即可重新构建。
这是 developer preview 的可复现依赖流程，还不是稳定 API、预编译安装包或硬件认证。
当前未发布 crates.io 包，请勿使用 `cargo install edgehome-kit`。

## 首次安装

准备 Git、[rustup](https://rustup.rs/) 和本机链接工具链。
Linux 需要 C 编译器/链接器；Windows 可使用 MSVC Build Tools，或已配置的 GNU 工具链；
macOS 需要 Command Line Tools。首次安装需要访问 GitHub、crates.io 和 Rust 分发服务。

```sh
git clone https://github.com/yushui2022/EdgeHome-Harness.git
cd EdgeHome-Harness
git rev-parse HEAD
cargo install --locked --path crates/edgehome-kit --bin edgehome-dev
edgehome-dev --version
edgehome-dev init ../my-edge-agent --name my-edge-agent
cd ../my-edge-agent
cargo build --locked
cargo run --locked -- doctor --mock
```

记录 `git rev-parse HEAD` 的完整提交号。首次 clone 取得当时的代码快照；复现该次安装时，
应先 `git checkout --detach <已记录的完整提交号>`，再运行安装命令。仓库与生成应用的
`rust-toolchain.toml` 固定 Rust **1.95.0**。将 Cargo 安装目录的 `bin` 加入 PATH；
可用 `cargo install ... --root <安装目录>` 自选位置。

CLI 的 `--version` 显示默认 SDK 基线提交号。该基线由版本库中的
[`sdk-revision.txt`](../crates/edgehome-kit/sdk-revision.txt) 显式维护，目前为
`7d10524dddd9aa1ff50678f929660afd828db4a4`，与 CLI 自身的安装提交分开记录。
默认 `init` 将这个已发布、与模板兼容的提交写入两个 SDK 依赖，
以内置的 SDK 锁文件为起点，通过 `cargo update --workspace` 保留已有依赖版本并完成应用锁定。
它会访问依赖源，但不会安装模型或启动推理服务。锁定失败时返回失败状态并保留项目，
修复网络或提交号后，在生成目录运行 `cargo update --workspace`，再构建；不需要重新初始化。

若只是阅读源码或临时试用，仍可在仓库执行 `cargo run --locked -p edgehome-kit -- init ...`。
安装后的 CLI 内置模板和依赖锁定基线，运行它不需要原仓库目录。

### 已知提交的固定安装

从项目选定一个已发布、经过检查的 **40 位提交号**，替换下面的占位符：

```sh
git fetch origin
git checkout --detach <CLI_COMMIT>
cargo install --locked --path crates/edgehome-kit --bin edgehome-dev --force
```

以上命令在 CLI 源码仓库中运行；占位符是记录或选定的 CLI 安装提交。
Rustup 按该检出目录的工具链文件选择编译器，首次需要下载对应版本。
固定源码检出后使用 `--path` 安装是这里验证的流程：它在共享 Cargo 构建目录下也会
检查源码和基线文件的更新。测试中直接 `cargo install --git` 复用共享构建目录时
出现过旧基线残留，因此本指南统一使用检出后的源码安装。
不要将 `main`、移动分支或仅 `0.1.0` 包版本当作 SDK 的唯一版本标识。
离线使用 `init --offline`，前提是对应 Git 提交及依赖索引已在 Cargo 缓存中。

## 应用迁移到另一台机器

把这些内容一起纳入应用自己的 Git 仓库：

| 文件 | 固定或保存什么 |
| --- | --- |
| `Cargo.toml` | SDK 仓库地址和相同的完整 `rev` |
| `Cargo.lock` | 解析后的直接/间接依赖版本及注册表包校验值 |
| `rust-toolchain.toml` | 编译器版本 |
| `src/`、`agent.yaml`、`cases.json` | 工具实现、无密钥配置、应用回归用例 |
| `UPGRADING.md` | 随应用携带的迁移、升级和回退步骤 |

在新机器准备相同工具链后，clone **应用仓库**，运行 `cargo build --locked`、
`cargo run --locked -- doctor --mock`，以及应用的固定用例。Cargo 自行获取固定提交的 SDK；
不用复制旧机器的 SDK 目录、修改绝对路径或安装全局 `edgehome-dev` 才能构建应用。

`--locked` 遇到缺失或需改变的锁文件会失败。普通构建不应删除锁文件或执行无范围的
`cargo update`。工具链、OS/ABI、链接器和外部原生库仍会影响产物；这里承诺依赖可复现，
不承诺不同平台生成逐字节相同的二进制。

离线设备应预先准备工具链、Git 源码缓存和依赖：先在相应环境执行 `cargo fetch --locked`，
随后 `cargo build --frozen`。完全隔离环境可另行使用 Cargo vendoring，但仍需准备 Rust
工具链与本机链接器。复制应用源码本身不等于复制构建缓存。

## 兼容性范围

| 层面 | 当前约定 |
| --- | --- |
| Rust | 最低版本声明 1.95；默认固定 1.95.0。更高版本需自行验证，不随 stable 自动升级 |
| 平台 | 需要 Rust `std` 和进程/网络能力；Linux、Windows 有安装与生成应用检查。macOS、ARM64、交叉编译仍需目标环境验证 |
| Windows 工具链 | 本地验证使用 x86_64 GNU；CI 增加 Windows 默认 MSVC 的安装/迁移检查 |
| Rust API / 配置 | developer preview，可随提交变化；应用必须固定提交并检查升级差异 |
| stdio | 启动握手声明协议 v1 和 capabilities；宿主核对版本与能力，不能假定新增命令已支持 |
| 模型与推理引擎 | 独立安装和固定版本；记录模型 digest、量化、聊天模板、引擎版本与配置 |

`doctor --probe` 验证两次受限指令探测。它和 mock 回归均不能证明实际模型任务可靠性；
部署前仍需运行应用自己的真实模型用例。受限设备可在开发机编译，但必须为目标 OS、架构、
ABI 和运行库生成产物。模型、凭证、数据目录及工具所需服务不由 Cargo 迁移。

## 升级与回退

CLI 与应用 SDK 分开升级。检出目标 CLI 提交，重新运行上面的 `cargo install ... --force` 可以替换 CLI；
它不会改写已有应用的工具代码、依赖或配置。若需并行保留多个 CLI，使用不同的 `--root` 目录。

应用升级时：

1. 先提交一个可运行基线，阅读目标提交的 [Changelog](../CHANGELOG.md) 与兼容性说明。
2. 将 `Cargo.toml` 中 `edgehome-kit`、`edgehome-runtime`，以及直接引用的
   `edgehome-inference` 的 `rev` 一起改为同一个目标提交。检查目标工具链要求，必要时同步固定版本。
3. 执行 `cargo update --workspace`，审查 `Cargo.lock` 差异，然后执行：

```sh
cargo build --locked
cargo test --locked
cargo run --locked -- doctor --mock
cargo run --locked -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
```

以上 eval 对应默认模板；已替换业务工具的应用改用自己的用例和授权工具列表。
随后检查真实推理服务与模型回归，最后把依赖、锁文件、工具链和 API/配置迁移作为一次应用提交。

回退时检出**完整的上一版应用提交**并 `cargo build --locked`，而非仅把 SDK 的 rev 改回去。
部署时保留旧二进制；工具已经修改的外部数据需由业务自己的迁移/回退机制处理。

旧应用的本地路径依赖可按 [模板迁移说明](../crates/edgehome-kit/templates/UPGRADING.md)
转换成 `git + rev`，保留原锁文件并完成上述验证。`init` 拒绝覆盖已有目录。

## 修改 SDK 的开发模式

```sh
edgehome-dev init ../sdk-lab --name sdk-lab --sdk-path /path/to/EdgeHome-Harness
```

此模式明确保留绝对路径依赖，适合联调未提交的 SDK 代码，不适合作为对外交付配置。
`--sdk-path` 与 `--sdk-rev` 互斥。自定义 `--sdk-rev` 只接受完整提交，且该提交必须已存在于
官方远端并与模板 API 兼容。默认基线不会随本地 HEAD、源码路径或编译缓存变化；
需要验证当前未发布 SDK 改动时，选择路径模式。

安装回归脚本 [`check-sdk-onboarding.py`](../scripts/check-sdk-onboarding.py) 验证 CLI 安装、
两种依赖模式、锁定版本、目录迁移、离线构建、升级/回退、两个工具、Python RPC 与 8 条用例。
Git 集成测试使用当前工作树建立临时远端，不修改全局 Git 配置，也不把 GitHub 网络可用性当作测试结论。

依赖行为依据 Cargo 的 [Git dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-dependencies-from-git-repositories)、
[`cargo install`](https://doc.rust-lang.org/cargo/commands/cargo-install.html) 和
[`cargo update --workspace`](https://doc.rust-lang.org/cargo/commands/cargo-update.html) 文档。

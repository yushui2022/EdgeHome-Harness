# CLI 下载、校验与版本管理

`edgehome-dev` 提供带版本号的独立 CLI 发行流程。发布的归档包包含可执行文件、
`agent.yaml`、8 条入门用例 `cases.json`、Python 客户端、MIT 许可、依赖许可文本和
`release.json`。CLI 内置文档统计与传感器示例工具，适合先体验调用流程。

**当前准备版本为 `v0.2.0`。只有 [Releases](https://github.com/yushui2022/EdgeHome-Harness/releases)
实际列出的版本才有可下载产物；新增发布流程本身不代表已经发布。** 所有发行暂标记为
developer preview，Rust API 和配置还没有稳定性承诺。

## 平台选择

以下为发布工作流的构建与运行检查矩阵；每个公开版本必须全部通过。
以该版本的 Release 日志和资产为准，不把配置了构建任务当作测试已通过。

| 平台 | 文件名中的 target | 构建与 smoke 环境 | 格式 |
| --- | --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` | Windows Server 2022；Windows 10/11 仍需部署验证 | `.zip` |
| Linux x64 | `x86_64-unknown-linux-gnu` | Ubuntu 22.04 / glibc 2.35 | `.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | Ubuntu 22.04 ARM64 / glibc 2.35 | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 14 ARM64 | `.tar.gz` |

Linux 包面向 GNU/glibc，不适用于 Alpine/musl、32 位 ARM 或无 `std` 的 MCU。
macOS 包未做 Developer ID 签名或公证；Windows 包未做 Authenticode 签名，系统可能提示
未知开发者。其他系统可使用源码安装。ARM64 runner 上的 mock 测试不代表真实端侧
模型的内存、速度或任务质量验收。矩阵来源是仓库的 `configs/distribution.json`。

## 下载并校验

从同一个 Release 下载对应归档及同名 `.sha256` 文件，或完整的 `SHA256SUMS`。
SHA-256 用于发现下载损坏或文件变化，不代替发布者签名。下面以已发布的 `v0.2.0`
为示例；若该版本尚不存在，先使用源码安装，不要把占位版本当作可用下载。

Windows PowerShell（在下载目录运行）：

```powershell
$archive = 'edgehome-dev-v0.2.0-x86_64-pc-windows-msvc.zip'
$checksum = (Get-Content -LiteralPath "$archive.sha256" -Raw).Trim()
$expected = ($checksum -split '\s+')[0]
if ($expected -notmatch '^[0-9a-f]{64}$') { throw 'Invalid checksum file' }
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Checksum mismatch; do not extract this archive'
}
# 使用新的版本目录，保留已有版本供回退。
Expand-Archive -LiteralPath $archive -DestinationPath .\edgehome-dev-v0.2.0
Set-Location .\edgehome-dev-v0.2.0
.\edgehome-dev.exe --version
.\edgehome-dev.exe doctor --mock
.\edgehome-dev.exe run --mock --prompt "Count the sample document" --tool document.stats --allow document.stats
```

Linux（ARM64 替换 target；macOS 替换 target 并使用 `shasum -a 256 -c`）：

```sh
archive=edgehome-dev-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
sha256sum -c "$archive.sha256" &&
mkdir edgehome-dev-v0.2.0 &&
tar -xzf "$archive" -C edgehome-dev-v0.2.0 &&
cd edgehome-dev-v0.2.0 &&
./edgehome-dev --version &&
./edgehome-dev doctor --mock &&
./edgehome-dev run --mock --prompt "Count the sample document" --tool document.stats --allow document.stats
```

这些命令不需要 Rust、Git、Python 或模型。`eval --mock --cases cases.json --allow document.stats
--allow sensor.read` 可运行包内 8 条用例。连接真实推理服务时编辑 `agent.yaml`，填入实际
模型名，然后运行 `doctor --probe`；模型和推理引擎需要另行安装。

如果希望在其他目录使用 CLI，把解压目录加入用户 PATH。配置和数据路径按当前工作目录
解析；可以用 `--config /absolute/path/agent.yaml` 明确选择配置。没有自动更新或系统服务。

## 开发自己的应用

预编译 CLI 的 `init` 内置项目模板，但生成 `Cargo.lock` 与编译自定义工具仍需要
**Git、Rust 1.95.0 和本机链接器**。工具代码在应用内静态编译，预编译 CLI 不会动态加载
应用源码。安装工具链后：

```sh
edgehome-dev init my-edge-agent --name my-edge-agent
cd my-edge-agent
cargo run --locked -- doctor --mock
```

应用继续固定已发布的 SDK commit，不会因下载新版 CLI 而自动升级。
应用依赖的安装、离线准备和迁移见
[安装与升级指南](https://github.com/yushui2022/EdgeHome-Harness/blob/main/docs/install-and-upgrade.md)。

## CLI 升级与回退

1. 选择明确版本，阅读该版本说明，下载匹配 OS/CPU 的归档并校验。
2. 解压到新的版本目录，执行 `--version`、`doctor --mock` 和包内用例。
3. 将 PATH 或启动脚本切换到新目录；保留旧目录和应用自己的配置。
4. 失败时切回旧目录。不要覆盖应用的 `agent.yaml`，也不要修改应用 SDK 的 `rev`。

记录版本包里的 `release.json`：它包含 CLI 版本、源代码 commit、默认 SDK commit、
Rust 编译器、目标平台、Cargo.lock 哈希和包内文件哈希。CLI 和 SDK 版本独立管理。
归档排序和时间戳经过规范化，但不承诺不同工具链/系统生成逐字节相同的二进制。

## 维护者发布流程

发布入口为 `.github/workflows/release.yml`。手动运行只构建和验证，资产保留在该次
Actions run；推送 `v*` 标签才创建公开 GitHub **prerelease**。没有隐式创建标签或覆盖
旧 Release 的逻辑；已有版本不可通过重跑替换资产，修复应使用新版本号。

1. 更新 `crates/edgehome-kit/Cargo.toml` 的 CLI 版本、Cargo.lock 和 Changelog。
   SDK 基线只在兼容性验证后单独更新，不要求所有 SDK crate 跟随 CLI 改版本。
2. 运行 `python scripts/test-release.py` 和项目发布检查，提交代码并确认 CI 通过。
3. 对该提交创建并推送带注释的标签，例如 `git tag -a v0.2.0 -m "EdgeHome CLI v0.2.0"`，
   再执行 `git push origin v0.2.0`。标签必须与 CLI 包版本完全一致。
4. 工作流复用完整 CI，并在四个平台测试 SDK、构建 CLI、打包、解压并运行原生 smoke。
   smoke 清空 PATH，验证没有 Rust/Git 时仍能运行版本查询、诊断、两种工具、8 条用例和 RPC。
5. 汇总任务核对平台是否齐全、归档及包内哈希、源码/SDK 版本，再生成 `SHA256SUMS`、
   `release-manifest.json` 和下载说明；任一门槛失败都不创建 Release。

本地可用 `python scripts/release.py package --target TARGET --binary PATH --output NEW_DIRECTORY`
演练打包。未提交源码须显式添加 `--allow-dirty`，产生的包标记为不可发布。
Windows GNU 仅支持本地演练，不在公开矩阵中。构建缓存、临时目录和归档输出应指向
开发机的数据盘；本项目 Windows 开发约定为 `G:\DevCache` 和 `G:\Projects\edgehome\release`。

工作流使用 GitHub 的[原生运行环境](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
和 [`gh release create --verify-tag`](https://cli.github.com/manual/gh_release_create)。

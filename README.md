# D2R Audio Mod

独立、轻量的 D2R 音频遥测 Mod 生成/加工工具。它只读取游戏资源或源 Mod，输出一个新 Mod 与 v7 协议清单；不读取 D2RHub 配置、账号、数据库，也不会启用 Mod。

生成器也可作为独立 sidecar 被接收软件调用。调用方必须显式传入游戏目录、源 Mod 与输出名称；生成器仍不会自行读取或修改调用方配置。使用 `--events` 时，标准输出会逐行返回 `progress`、`completed` 或 `error` JSON 事件，便于显示真实进度。

## 简单界面

直接双击 `d2r-audio-mod.exe`。界面只有三个输入：

- **源 MOD**：可留空；留空时生成独立最小 MOD，选择后则保留源 MOD 内容并附加声纹。
- **生成的 MOD 名称**：默认 `D2RAudioTelemetry`，只使用英文字母、数字、`-`、`_`。
- **输出目录**：选择游戏安装目录中的 `mods` 文件夹最省心；如果选择游戏安装目录本身，界面会自动改为其 `mods` 子目录。

点击“生成 MOD”后固定生成全区域、全部支持物品和主界面声纹，不提供容易混淆的高级参数。完成后界面会显示准确的 `-mod ... -txt` 启动参数，并可直接打开生成目录。源 MOD 永远不会被覆盖。

加工现有 Mod 时不要求它包含完整游戏数据表。`misc.txt`、`sounds.txt`、`levels.txt` 与 `soundenviron.txt` 会逐个解析：源 Mod 提供的版本优先保留，缺失的文件才从 `--game` 指向的本机 D2R 数据中补齐。因此只改动少量数据表的轻量 Mod 也可以直接加工。

HD 实体、状态机和物品映射优先按标准 JSON 读取；若源 Mod 使用注释、未加引号的键、单引号或尾逗号等 JSON5 写法，加工器会兼容读取并将实际改写的文件输出为标准 JSON。资源内容不会因为宽松语法而被跳过或替换成原版。

## 命令行

常用示例：

```powershell
# 从本机游戏创建最小 Mod（默认全地图、全部类别）
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected"

# 自定义生成名称；目录、.mpq 名称和 -mod 参数都会使用此名称
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --name "MyAudioMod"

# 在 jcy Mod 上附加声纹，输出到游戏 mods 目录
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --game "C:\Program Files (x86)\Diablo II Resurrected"

# 只加工符文、钥匙和器官
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --track runes,keys,organs
```

运行 `d2r-audio-mod.exe help` 查看完整参数。生成完成后，按工具输出的 `-mod ... -txt` 参数自行启用新 Mod。

`--name` 只接受 ASCII 字母、数字、连字符和下划线，例如 `MyAudioMod` 或 `jcy-Audio`。若同名目录已存在，工具仍不会覆盖，而会自动生成 `-2`、`-3` 等后缀；请以本次输出的启动参数为准。

加工扩展物品时，工具为每个代码克隆独立实体和状态机，并同步复制其普通/低配背包 sprite；这既保留模型与物品图标，也允许原本共用同一实体的物品使用不同声纹。主界面同时覆盖五幕前端场景、营火、选角循环、前端事件与选项音乐入口。

## 仓库边界

- `src/` 只包含 Mod 的创建、加工和命令行入口。
- `crates/d2r-audio-protocol/` 是生产端使用的版本化音频协议实现。
- `protocol/` 保存生产端与任意接收端都可实现的 v7 格式约定。
- 本仓库不引用 D2RHub 源码或本机的 D2RHub 仓库路径；接收端只需实现同一协议并读取生成 Mod 内的清单。
- 接收软件可以打包并调用编译后的生成器，但账号选择、启动参数修改和结果复核必须由接收软件自行完成；这不改变两个代码库的独立性。

## 构建与验证

```powershell
cargo test
cargo build --release
```

发布文件为 `target/release/d2r-audio-mod.exe`。仓库不包含生成出的 Mod、游戏资源或构建目录。

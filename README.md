# D2R Audio Mod

当前版本 **v1.3.1**。这是独立、轻量的 D2R 音频遥测 Mod 生成/加工工具。它只读取游戏资源或源 Mod，输出一个新 Mod 与 v7 协议清单；不读取 D2RHub 配置、账号、数据库，也不会启用 Mod。

生成器也可作为独立 sidecar 被接收软件调用。调用方必须显式传入游戏目录、源 Mod 与输出名称；生成器仍不会自行读取或修改调用方配置。使用 `--events` 时，标准输出会逐行返回 `progress`、`completed` 或 `error` JSON 事件，便于显示真实进度。

## 简单界面

直接双击 `d2r-audio-mod.exe`。界面只有三个输入：

- **源 MOD**：可留空；留空时生成独立最小 MOD，选择后则保留源 MOD 内容并附加所选功能。
- **生成的 MOD 名称**：默认 `D2RAudioTelemetry`，只使用英文字母、数字、`-`、`_`。
- **输出目录**：选择游戏安装目录中的 `mods` 文件夹最省心；如果选择游戏安装目录本身，界面会自动改为其 `mods` 子目录。

独立 GUI 仍默认生成全区域声纹和局内房间工具；D2RHub 或命令行可用 `--features audio`、`--features rooms`、`--features death-exit` 或逗号组合按需选择。`death-exit` 属于高影响的显式选项，不包含在默认 `all` 中。r25 清单会按功能组记录独立版本和参数指纹。把 r25 成品作为新一轮来源时，声纹版本、协议、覆盖范围、类别、增益、目录及全部文件均一致才会直接复用，不重新编码或覆盖 FLAC。源 MOD 永远不会被覆盖。

选择 `rooms` 时会加入局内房间工具栏：`下一局` 需要在 0.5 秒内左键双击，首次点击不会显示二级确认条；确认双击后先打开在线 `PauseLayoutGarden`，再按 JCY 的顺序发送 `PausePanelMessage:ExitGame` 与 `CharacterSelect:LoadCharacter:2`，通过正常退出路径开始下一局。`创建房间` 与 `加入房间` 保持单击打开表单；点击按钮、按回车或选择房间触发实际提交时，会先打开 `PauseLayoutGarden`，再依次发送 `PausePanelMessage:ExitGame` 与原生 `CreateGame:CreateGame` 或 `JoinGame:JoinGame`，避免主动换房被显示为连接中断。三个按钮缩至 0.30 倍并紧凑排列，同时保留暂停菜单中的无操作安全焦点；暂停菜单所有真实按钮的左右导航也会汇入创建/加入安全入口，避免鼠标悬停改变焦点。自动流程用 `Esc → 左/右两次 → 确认` 打开创建/加入表单，随后通过 F13 调用原生 `CfgChat` 文本态，Tab 切换密码并提交。

`rooms` 配方 r23 同时加入大厅背景的“按 Esc 键返回”提示，沿用 JCY 的暗金色、120 字号、水平/垂直居中样式及水平 -50 偏移。已有 JCY 提示会原位更新；其他大厅布局保留完整内容，并在背景下方加入屏幕锚点为 (0.5, 0.45) 的提示层。源 Mod 缺少大厅布局时从游戏数据补齐。重复加工不会叠加提示；这条提示依附大厅背景，不检测黑屏，也不改变 Esc 行为。

选择 `death-exit` 时会保留源 Mod 或原版的完整死亡界面，只移除旧式的 `PanelManager:OpenPanel:exitgame` 定时入口并追加具名入口。死亡弹窗出现 10ms 后打开 `D2RHubAutoExitOnDeath` 独立面板，再等待 100ms 发送 `PausePanelMessage:ExitGame`。重复加工不会叠加定时器，也不会覆盖游戏原生 `exitgamehd.json`。能力指纹不包含启停状态；加工已有 Mod 时会保留源 Mod 的实际启停状态，日常启停由 D2RHub 在具体 Mod 条目中只修改启动定时器。该功能只能在死亡判定后自动离开当前游戏，不能避免死亡、撤销惩罚或挽救专家模式角色。

加工始终采用“新产物”策略：先把源 Mod 复制到事务临时目录，再对数据表和布局做结构化合并，全部验证通过后才一次性提交为另一个 Mod。源 Mod 永不改写；同名输出已存在时也不覆盖，而是生成 `-2`、`-3` 等新名称。源内容与 D2RHub 功能没有冲突时原样保留；对 D2RHub 必须拥有的声纹字段、具名布局节点、输入框焦点和安全键盘入口，只在新产物内以当前配方值更新。任何必需资源无法解析时整次加工失败并丢弃未完成产物，不留下半成品。

加工现有 Mod 时不要求它包含完整游戏数据表。`misc.txt`、`sounds.txt`、`levels.txt` 与 `soundenviron.txt` 会逐个解析：源 Mod 提供的版本优先保留，缺失的文件才从 `--game` 指向的本机 D2R 数据中补齐。因此只改动少量数据表的轻量 Mod 也可以直接加工。

这四张源 Mod 数据表兼容 UTF-8（含 BOM）、UTF-16 LE/BE、GBK/GB18030 与常见 Windows ANSI 编码。生成器只在内存中解码，绝不改写源 Mod；新 Mod 中实际加工的数据表统一输出为 UTF-8。

HD 实体、状态机、物品映射、地图本地化与 `modinfo.json` 优先按标准 JSON 读取；若源 Mod 使用注释、未加引号的键、单引号或尾逗号等 JSON5 写法，加工器会兼容读取并将实际改写的文件输出为标准 JSON。资源内容不会因为宽松语法而被跳过或替换成原版。

源 Mod 用 0 字节 FLAC 表达静音时，加工器会保留静音语义并只写入周期声纹，不会尝试解码空文件或回退到游戏原声。

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

# 只生成局内房间工具，不生成任何声纹
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --features rooms

# 从已有 r22 声纹 Mod 补充房间工具；已验证且未变化的声纹直接复用
d2r-audio-mod.exe augment --source "D:\Mods\MyAudioR22" --game "C:\Program Files (x86)\Diablo II Resurrected" --features rooms --name "MyAudioAndRooms"

# 只加入死亡后自动退房；不会生成声纹或房间工具
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --features death-exit --name "DeathExitOnly"

```

运行 `d2r-audio-mod.exe help` 查看完整参数。生成完成后，按工具输出的 `-mod ... -txt` 参数自行启用新 Mod。

### 功能组与版本边界

r22 是独立功能组体系的起点。生成结果同时写入 `d2rhub-mod-manifest.json`，并保留同内容的 `audio-telemetry-manifest.json` 兼容别名。清单中的 `feature_groups` 是可扩展数组，而不是固定的“全功能版本号”；每一项包含：

- `id`：稳定功能标识，例如 `audio_telemetry`、`in_game_room_tools`、`auto_exit_on_death`。
- `recipe_version`：只在该组生成逻辑发生不兼容变化时递增。
- `fingerprint`：标识该组已安装能力的配方参数；运行时启停状态不写入能力指纹。
- `reused_from_source`：本次是否直接复用了已经完整核对的来源产物。

加工是“增加或更新所选组”，不是删除未选择组。以当前 r25 成品为来源时，来源中未选择、但清单有效的其他功能组及其未知未来组会原样保留；因此继续增加新功能组时，不需要改变现有清单结构或重做无关组。

r22 的声纹组仍使用旧的环境音首包配方；r23 将普通 Area 改为流式三连入场探针，并把 TZ 1023 明确定义为独立状态位；r24 的局内房间工具组 r20 进一步加固了暂停菜单焦点入口；r25 的声纹组 r3 会按原声音表音量预补偿区域背景声，避免统一的 255 声纹播放链路放大 A4 等区域的环境音，同时保持声纹本身的输出强度，局内房间工具组 r22 则让重开、创建和加入都先经正常退出路径提交。请从原版或当初未经加工的原始 Mod 生成一个新名称的 r25 Mod，之后的二次加工才能安全复用未变化的组。

### 声纹复用判定

声纹组只有在以下内容全部一致时才复用：声纹组配方版本、v7 协议、区域覆盖、跟踪类别、增益参数、区域/物品目录，以及清单声明的每个声纹文件。任一项缺失或变化都会拒绝复用。复用成功时不会重新编码、覆盖或用新空文件替换已有 FLAC；生成报告会把 `audio_telemetry.reused_from_source` 标为 `true`。

如果只需要局内房间工具，应从原版或未加工 Mod 选择 `--features rooms`；产物不会生成声纹、区域目录或物品目录。如果以已有 r22 声纹 Mod 为来源再加入 `rooms`，原声纹会保留并复用，而不是被删除。

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

Windows 下游戏安装目录、源 Mod 与输出目录均支持中文、空格及 Windows 允许的特殊字符。新 Mod 名称仍仅允许 ASCII 字母、数字、`-` 和 `_`，以兼容 D2R 的启动参数与 Mod 目录约定。

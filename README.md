# D2R Audio Mod

本项目是 D2RHub 内置的 Mod 加工器源码。生成和加工读取源 Mod 与游戏资源，输出独立结果；MPQ 解压会备份原包并进行可恢复转换。加工器不读取 Hub 账号或数据库。

加工器随 D2RHub 安装包发布，不再独立发布 EXE，也不做 Hub 软件版本互认。功能兼容性按模块协议校验。LiteHub、BoHub、NullHub 的内置生成仍已移除，已有成品可作为源 Mod。

## 当前开发变更

- 整体成品配方 r26、声纹组 r4：只加工掉落声音并输出完整 OCR 地区目录，不生成地区、TZ 或主界面声音；旧场景声纹成品须从原始来源重做。
- 新增可选敌对重开、玩家尸体拾取按钮和场景重开，默认 `all` 仍仅包含 audio + rooms。
- MPQ 解压、恢复与验证工具保留。
- 创建保留原 10ms 打开暂停、50ms 退出/提交/关闭，并默认地狱。
- 房间模块 r33：加入按钮和回车直接发送 `JoinGame:JoinGame`，没有退出中转。
- 双 Esc r4：恢复基准 HUD 清理行为。
- 事件 CLI 无需调用方身份；模块结构与版本由使用方检查。

## 历史：v1.4.0-beta.18 局内创建默认地狱

- 局内创建表单打开后 50ms 发送一次 `CreateGame:SetDifficulty:2`，保留房名输入焦点及之后手动切换难度的能力。该初始化从房间工具 r29 引入，当前 r30 同时包含创建／加入的延迟提交调整。
- 默认难度节点只加入 `D2RHubInGameCreateGamehd.json`；大厅与加入表单不注入该节点。重复加工保持单个节点，旧成品需重新加工并重启游戏。
- 生成、结构校验不能代替实机验收：需验证首次及重复打开、手动改难度、未解锁地狱的角色，以及实际房间难度。
- 发布时应使用此版本重新构建加工器和配套成品，再由 D2RHub 发布流程生成真实的下载地址、大小及 SHA-256。当前已发布资源目录仍对应旧版，不能只改版本标签冒充新产物。

## v1.3.4 更新

- 兼容源 Mod 用空白、BOM 或纯注释 JSON 屏蔽符文/物品实体、落地状态机和物品映射的情况。需要本机游戏数据作为基线；只为选中的声纹功能构建无模型实体和私有状态机，保留共享屏蔽文件及未选物品的裁剪策略。
- 必需骨骼或动画也被置空时，将游戏基线复制到声纹专用路径，不恢复共享资源。被屏蔽状态机的原声音与视觉事件不会恢复。
- 正常 JSON/JSON5 继续保留源定义；精简实体缺少依赖列表时只补状态机引用。非空语法错误、类型错误及不支持的状态转场仍明确失败，不通过恢复整套原版资源掩盖问题。
- 保持现有 v7 协议、声纹组 r3 和成品复用规则不变。本次覆盖生成和资源结构检查；无模型实体在游戏内的实际声纹触发仍需运行验证。

## v1.3.3 更新

- 发布独立 Windows 可执行文件，并公开生成器及 v7 协议源码；版本号与 D2RHub 分别维护。
- 包含房间工具配方 **r28**：大厅使用原生创建/加入表单，局内使用独立表单，退出与下一步操作沿用 JCY 的同一时点顺序提交。
- 从本机游戏重建两份高清暂停菜单，三个局内按钮始终隐藏，双击 Esc 下一局地狱改为独立可选模块；旧房间工具升级时更新局内表单，未变化且验证完整的声纹组可复用。
- 本次发布保持声纹 v7 协议、整体产物 r25 和声纹组配方版本不变，房间工具组升级为 r28；生成器软件版本不等于产物或功能组版本。

生成器也可作为独立 sidecar 被接收软件调用。调用方必须显式传入游戏目录、源 Mod 与输出名称；生成器仍不会自行读取或修改调用方配置。标准输出会逐行返回 `progress`、`completed` 或 `error` JSON 事件，便于显示真实进度。

## 简单界面

局内“下一局、创建房间、加入房间”三个按钮始终不可见，不再提供显示开关，也不继承旧成品的显示选择。独立选择 `esc-next-game` 后，局内 0.5 秒内双击 Esc 进入下一局地狱；单击打开暂停菜单，超时后 Esc 正常返回游戏。`Esc → 左/右两次 → 回车` 仍可创建/加入房间，D2RHub 后台跟房使用此入口。旧成品需重新加工并重启游戏。

直接双击 `d2r-audio-mod.exe`。界面只有三个输入：

- **源 MOD**：可留空；留空时生成独立最小 MOD，选择后则保留源 MOD 内容并附加所选功能。
- **生成的 MOD 名称**：默认 `D2RAudioTelemetry`，只使用英文字母、数字、`-`、`_`。
- **输出目录**：选择游戏安装目录中的 `mods` 文件夹最省心；如果选择游戏安装目录本身，界面会自动改为其 `mods` 子目录。

独立 GUI 默认生成掉落声纹、完整 OCR 地区目录和局内房间工具；D2RHub 或命令行可用 `--features audio`、`--features rooms`、`--features esc-next-game`、`--features hostile-restart`、`--features scene-shortcuts`、`--features party-loot`、`--features death-exit` 或逗号组合按需选择。双 Esc、敌对重开、场景重开、拾取按钮与死亡退房均需显式选择，不包含在默认 `all` 中。r26 清单按功能组记录独立版本和参数指纹。音频组 r4 不再生成地区、TZ、主界面声音，仅保留掉落，敌对提示由独立的 `hostile-restart` 模块处理；场景交给 D2RHub OCR。旧场景音成品必须从未加工原始来源重做，不能作为干净基线。相同新版声纹组在参数和全部文件校验一致时可以复用。源 MOD 永远不会被覆盖。

选择 `rooms` 时只加入隐藏的创建/加入入口；选择 `esc-next-game` 才启用双击 Esc 下一局地狱，两者可独立或组合使用。当前房间工具 r33 保留大厅与局内表单分离：大厅恢复原生提交；局内创建通过 10ms 打开暂停、50ms 排队退出／创建／清理，默认地狱；局内加入的按钮和回车直接发送 `JoinGame:JoinGame`，不走退出控制器。独立双 Esc r4 保留暂停菜单的 0.5 秒接收窗口，超时后正常返回游戏。

局内房间工具配方 r25 将 `pauselayouthd.json` 与 `pauselayoutgardenhd.json` 整份替换为从当前 D2R 游戏原版重建的布局，再注入房间工具。源 Mod 在这两份文件中的自定义外观、按钮、Esc 快捷行为、定时器和消息链均不再继承；独立模块 `esc-next-game` 在暂停菜单打开后启用 0.5 秒的隐藏 Esc 接收入口，触发下一局地狱；超时后 Esc 通过 `ReturnToGame` 返回游戏，点击其他原生菜单动作会先清除双击状态。加工 `rooms` 必须能读取有效的 D2R 游戏目录（含 `.build.info` 与 `Data`）；原版数据不可用时明确报错，不回退到源 Mod 的暂停布局。

三个工具栏按钮始终隐藏，暂停菜单保留无操作安全焦点，所有原版按钮的左右导航汇入创建/加入安全入口。自动流程用 `Esc → 左/右两次 → 确认` 打开表单，随后通过 F13 调用原生 `CfgChat` 文本态，Tab 切换密码并提交。已有 r21-r31 房间工具需要重新加工升级，D2RHub 仍按旧配方读取它们作为升级来源。重建范围仅为上述两份高清暂停布局，其他文件中的界面或外部按键脚本不属于这项接管范围。

`rooms` 配方 r23 同时加入大厅背景的“按 Esc 键返回”提示，沿用 JCY 的暗金色、120 字号、水平/垂直居中样式及水平 -50 偏移。已有 JCY 提示会原位更新；其他大厅布局保留完整内容，并在背景下方加入屏幕锚点为 (0.5, 0.45) 的提示层。源 Mod 缺少大厅布局时从游戏数据补齐。重复加工不会叠加提示；这条提示依附大厅背景，不检测黑屏，也不改变 Esc 行为。

选择 `death-exit` 时会保留源 Mod 或原版的完整死亡界面，只移除旧式的 `PanelManager:OpenPanel:exitgame` 定时入口并追加具名入口。死亡弹窗出现 10ms 后打开 `D2RHubAutoExitOnDeath` 独立面板，再等待 100ms 发送 `PausePanelMessage:ExitGame`。重复加工不会叠加定时器，也不会覆盖游戏原生 `exitgamehd.json`。能力指纹不包含启停状态；加工已有 Mod 时会保留源 Mod 的实际启停状态，日常启停由 D2RHub 在具体 Mod 条目中只修改启动定时器。该功能只能在死亡判定后自动离开当前游戏，不能避免死亡、撤销惩罚或挽救专家模式角色。

加工始终采用“新产物”策略：先把源 Mod 复制到事务临时目录，再对数据表和布局做结构化合并，全部验证通过后才一次性提交为另一个 Mod。源 Mod 永不改写；同名输出已存在时也不覆盖，而是生成 `-2`、`-3` 等新名称。源内容与 D2RHub 功能没有冲突时原样保留；对 D2RHub 必须拥有的声纹字段、具名布局节点、输入框焦点和安全键盘入口，只在新产物内以当前配方值更新。任何必需资源无法解析时整次加工失败并丢弃未完成产物，不留下半成品。

加工现有 Mod 时不要求它包含完整游戏数据表。`misc.txt`、`sounds.txt` 与供 OCR 地区目录使用的 `levels.txt` 会逐个解析：源 Mod 提供的版本优先保留，缺失的文件才从 `--game` 指向的本机 D2R 数据中补齐。因此只改动少量数据表的轻量 Mod 也可以直接加工。

这四张源 Mod 数据表兼容 UTF-8（含 BOM）、UTF-16 LE/BE、GBK/GB18030 与常见 Windows ANSI 编码。生成器只在内存中解码，绝不改写源 Mod；新 Mod 中实际加工的数据表统一输出为 UTF-8。

HD 实体、状态机、物品映射、地图本地化与 `modinfo.json` 优先按标准 JSON 读取；若源 Mod 使用注释、未加引号的键、单引号或尾逗号等 JSON5 写法，加工器会兼容读取并将实际改写的文件输出为标准 JSON。资源内容不会因为宽松语法而被跳过或替换成原版。

源 Mod 用 0 字节 FLAC 表达静音时，加工器会保留静音语义并只写入周期声纹，不会尝试解码空文件或回退到游戏原声。

## 命令行

常用示例：

```powershell
# 从本机游戏创建最小 Mod（默认完整 OCR 地区目录、全部掉落类别）
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected"

# 自定义生成名称；目录、.mpq 名称和 -mod 参数都会使用此名称
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --name "MyAudioMod"

# 在 jcy Mod 上附加声纹，输出到游戏 mods 目录
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --game "C:\Program Files (x86)\Diablo II Resurrected"

# 只加工符文、钥匙和器官
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --track runes,keys,organs

# 只生成局内房间工具，不生成任何声纹
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --features rooms

# 从当前 r26／声纹 r4 成品补充房间工具；已验证且未变化的声纹直接复用
d2r-audio-mod.exe augment --source "D:\Mods\MyAudioR26" --game "C:\Program Files (x86)\Diablo II Resurrected" --features rooms --name "MyAudioAndRooms"

# 只加入死亡后自动退房；不会生成声纹或房间工具
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected" --features death-exit --name "DeathExitOnly"

```

运行 `d2r-audio-mod.exe help` 查看完整参数。生成完成后，按工具输出的 `-mod ... -txt` 参数自行启用新 Mod。

### 功能组与版本边界

r22 是独立功能组体系的起点。生成结果同时写入 `d2rhub-mod-manifest.json`，并保留同内容的 `audio-telemetry-manifest.json` 兼容别名。清单中的 `feature_groups` 是可扩展数组，而不是固定的“全功能版本号”；每一项包含：

- `id`：稳定功能标识，例如 `audio_telemetry`、`in_game_room_tools`、`esc_next_game`、`auto_exit_on_death`。
- `recipe_version`：只在该组生成逻辑发生不兼容变化时递增。
- `fingerprint`：标识该组已安装能力的配方参数；运行时启停状态不写入能力指纹。
- `reused_from_source`：本次是否直接复用了已经完整核对的来源产物。

加工是“增加或更新所选组”，不是删除未选择组。以当前 r26 成品为来源时，来源中未选择、但清单有效的其他功能组及其未知未来组会原样保留；因此继续增加新功能组时，不需要改变现有清单结构或重做无关组。

r22–r25 是历史环境音场景配方。r26 不会把其残留场景／TZ／主界面声纹当作干净来源继续增补；应回到原版或未经加工的源 Mod 重新生成。当前 r4 声纹组参数和文件都一致时可复用，其他已知模块遇到未来版本或未知指纹时拒绝降级。生成器软件版本、整体成品配方和各功能组版本分别维护。

### 声纹复用判定

声纹组只有在以下内容全部一致时才复用：声纹组配方版本、v7 协议、区域覆盖、跟踪类别、增益参数、区域/物品目录，以及清单声明的每个声纹文件。任一项缺失或变化都会拒绝复用。复用成功时不会重新编码、覆盖或用新空文件替换已有 FLAC；生成报告会把 `audio_telemetry.reused_from_source` 标为 `true`。

如果只需要局内房间工具，应从原版或未加工 Mod 选择 `--features rooms`；产物不会生成声纹、区域目录或物品目录。如果以当前 r26／声纹 r4 成品为来源再加入 `rooms`，原声纹在完整校验通过后保留并复用；r22 旧声纹来源会拒绝增补。

`--name` 只接受 ASCII 字母、数字、连字符和下划线，例如 `MyAudioMod` 或 `jcy-Audio`。若同名目录已存在，工具仍不会覆盖，而会自动生成 `-2`、`-3` 等后缀；请以本次输出的启动参数为准。

加工扩展物品时，工具为每个代码克隆独立实体和状态机，并同步复制其普通/低配背包 sprite；这既保留模型与物品图标，也允许原本共用同一实体的物品使用不同声纹。当前不生成主界面声音，也不生成菜单结束场景的信号。

## 新增独立模块

| CLI 功能 | 功能组 | 行为与依赖 |
| --- | --- | --- |
| `hostile-restart` | `hostile_auto_restart` r1 | 保留敌对原音并加入 v7 控制标记 `type=11,id=1`；自动包含双 Esc。仅修改四条敌对声音映射，不依赖掉落或场景识别。 |
| `party-loot` | `party_loot` r2 | 合并 MYMOD／JCY 的原生玩家行拾取按钮；其他来源增加独立列表。无需图像识别，不自动授予所有玩家权限。 |
| `scene-shortcuts` | `scene_shortcuts` r7 | 需在 A3、崔凡克或 A4 小站处重开或创房。使用原生面板链并同步第三、四幕小站映射，替换帮助页并保留备份，无需图像识别。 |

场景重开需在 A3、崔凡克或 A4 小站处重开或创房；场景重开与左侧玩家尸体拾取按钮均使用游戏原生流程，无需图像识别或 OCR。实现与各轮调整见 [场景重开说明](docs/scene-shortcuts.md)。旧版 r1–r6 的已知指纹支持重建升级；未知未来配方拒绝降级覆盖。

```powershell
# 只加工敌对提示与双 Esc
.\target\release\d2r-audio-mod.exe minimal --game "C:\Diablo II Resurrected" --output "D:\ModBuild" --name HostileReview --features hostile-restart
# 组合房间工具、场景重开与原生拾取按钮
.\target\release\d2r-audio-mod.exe minimal --game "C:\Diablo II Resurrected" --output "D:\ModBuild" --name RoomTools --features "rooms,scene-shortcuts,party-loot"
```

在 Hub 中选择“同名加工源 Mod”时，生成器仍写入独立暂存目录，由 Hub 校验后保留原始备份并执行同名替换；生成器本身不会原位覆盖源目录。

## 仓库边界

- `src/` 只包含 Mod 的创建、加工和命令行入口。
- `crates/d2r-audio-protocol/` 是生产端使用的版本化音频协议实现。
- `protocol/` 保存生产端与任意接收端都可实现的 v7 格式约定。
- 本仓库不引用 D2RHub 源码或本机的 D2RHub 仓库路径；接收端只需实现同一协议并读取生成 Mod 内的清单。
- 接收软件可以打包并调用编译后的生成器，但账号选择、启动参数修改和结果复核必须由接收软件自行完成；这不改变两个代码库的独立性。

## 从源码构建

```powershell
git clone https://github.com/gjy991229/d2r-audio-mod.git
Set-Location d2r-audio-mod
cargo build --locked --release --locked
```

Windows 构建需要 Rust stable 的 MSVC 工具链、Visual Studio C++ Build Tools 与 Windows SDK。源代码自带协议实现，Cargo 会按锁文件下载依赖；构建不需要 D2RHub 仓库。生成 Mod 时仍需你自己的游戏安装或源 Mod 资源。

发布文件为 `target/release/d2r-audio-mod.exe`。仓库不包含生成出的 Mod、游戏资源或构建目录。

Windows 下游戏安装目录、源 Mod 与输出目录均支持中文、空格及 Windows 允许的特殊字符。新 Mod 名称仍仅允许 ASCII 字母、数字、`-` 和 `_`，以兼容 D2R 的启动参数与 Mod 目录约定。

## 开发验证

提交前运行 `cargo fmt -- --check`、`cargo clippy --all-targets --all-features -- -D warnings` 和 `cargo test`。

房间工具集成测试需要本机原版 CASC。设置 `D2RHUB_AUDIO_GAME_ROOT` 为游戏安装目录后，运行 `cargo test room_tools -- --ignored`，验证独立房间工具、声纹保留、原版暂停菜单和重复加工。测试仅向临时目录输出，不修改已安装 Mod 或启动游戏。缺少原版资源时拒绝加工的回归测试包含在默认测试中。

## English

D2R Audio Mod is the MIT-licensed processor bundled with the D2RHub installer. Source remains in this repository. Compatibility uses module protocols rather than paired application versions.

Double-click the executable for its standalone UI, or use `minimal` / `augment` with `--features audio`, `rooms`, `death-exit`, or a comma-separated selection. Death-triggered exit is an explicit opt-in and is excluded from the default feature set. The generator creates a new output Mod, preserves the source, and never reads D2RHub accounts or settings. Enable the generated Mod using the launch arguments shown in its result.

This processor ships inside D2RHub 0.9.112. Room-tools r33 keeps the create timing and submits joins through the native JoinGame message. Double-Esc r4 removes the persistent HUD receiver-close timer; see [the r33 contract](docs/room-tools-r33.md). Existing Mods must be regenerated and the game restarted. Audio protocol v7 and the overall r25 output format are unchanged. Build with `cargo build --release --locked` using Rust and the Windows MSVC build tools; see [protocol/](protocol/) for the receiver-independent audio specification.

Diablo II: Resurrected and Battle.net are trademarks of Blizzard Entertainment. This is an unofficial project; the MIT license covers this repository's code, not Blizzard game resources or third-party Mod assets.

常规验证包括生成器及独立协议 crate 的测试；依赖原始游戏资源的测试默认忽略，只显式运行时读取本地游戏数据。

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --manifest-path crates/d2r-audio-protocol/Cargo.toml
```

根目录 `Cargo.lock` 固定加工器依赖；独立协议库测试产生的局部锁文件只用于本机，不纳入源码。

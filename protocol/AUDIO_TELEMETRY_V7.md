# D2R Audio Telemetry Protocol v7

这是 Mod 生产端与音频接收端之间的内容协议。两端不共享源码、账号、配置或数据库；接收软件可以把生成器作为独立进程调用，但只能通过显式命令行输入与结构化输出协作。

## 产品边界

- 生产端读取本机 D2R 数据或一个源 Mod，输出新的 Mod、协议清单和建议启动参数。
- 加工源 Mod 时，生产端必须按文件保留其已有数据表；源 Mod 未覆盖的数据表可以从同版本本机 D2R 数据补齐，不能因为源 Mod 只包含部分覆盖表而拒绝加工。
- 生产端不得修改源 Mod、游戏账号、启动参数或接收端配置。
- 接收端只捕获目标 D2R 进程的音频、解码协议并统计。若接收软件负责易用性编排，它可以调用生成器，但必须自行完成账号选择、启动参数更新及产物复核。
- 接收端根据当前账号的 `-mod <name>`，在 `<game>/mods/<name>/` 查找清单。
- 双方只要求协议主版本完全相同。未知字段必须忽略；同一主版本内已有身份编号不得重排或复用。

## Mod 根目录文件

- `audio-telemetry-manifest.json`：生产报告；`protocol_version` 判断能否解码，`recipe_version` 判断是否建议重新生成，其他未知字段必须忽略。
- `audio-telemetry-area-catalog.json`：Area Id 到场景名称、稳定键和地点类型的映射。
- `audio-telemetry-item-catalog.json`：物品协议 Id 到基础物品代码、类别和显示名的映射。

旧版文件名 `rune-audio-area-catalog.json` 与 `d2rhub-audio-item-catalog.json` 仅作为接收端兼容输入；新生产端不得再生成。

## 声音编码

- 最低采样率：44,100 Hz；推荐 48,000 Hz。
- 默认声纹增益：-30 dBFS；合法范围 -42 到 -12 dBFS。
- 每个标记包含 63 chip 同步码和负载，写入两份相同数据包用于单次播放内校验。
- 地点/主界面：20 bit 负载；`2 bit magic + 2 bit type + 10 bit id + CRC-6`。
- 掉落：独立同步码加 127 chip Gold 签名，避免地点包进入掉落解码器。
- 载波：同步码 18,000 Hz；掉落签名 19,600 Hz；地点数据位 17,000/19,000 Hz。
- CRC：`x^6 + x + 1`，省略最高项后为 `0x03`。

## 身份空间

- `type=00`：扩展物品，Id 1–94。
- `type=01`：符文，编号 1–33。
- `type=10`：地图，D2R Area Id 1–1023。
- `type=11,id=0`：主界面。
- 掉落 Gold 签名号：符文使用 1–33；扩展物品使用 `33 + item_id`，即 34–127。

协议内置 50 个扩展物品身份和 7 个类别：`runes,gems,charms,jewels,keys,organs,essences`。具体稳定编号以 `d2r-audio-protocol` 的 `SUPPORTED_ITEMS` 为准。

## 清单兼容规则

- `protocol_version` 是硬兼容边界：不匹配时不能启动接收；`recipe_version` 是生成内容版本，只在缺少当前必需资源时递增。
- 早期正式清单没有 `recipe_version`，接收端必须继续允许其原有识别能力，同时可以非阻断地提示生成新版。
- 高于接收端已知值的 `recipe_version` 在协议主版本相同时按兼容处理，避免软件降级产生误报。
- 新清单可记录 `source_mod_name` 作为下次生成的选择提示；它不是可信路径，接收端使用前必须重新验证对应 Mod。
- 两份目录文件都必须含 `protocol_version: 7`。
- Area 条目必须含 `area_id`、`scene_key`、中英文名称和 `kind`；`kind` 为 `town`、`wilderness` 或 `frontend`。
- Item 条目必须含 `item_id`、D2R 基础代码 `code`、`category`、中英文名称和资源路径 `asset`。
- 接收端遇到缺失/无效/版本不匹配的地图清单时，只使用内置地点回退；物品显示名使用协议内置回退。
- 清单只描述身份和显示信息，不是控制通道，不能要求接收端写回任何内容。

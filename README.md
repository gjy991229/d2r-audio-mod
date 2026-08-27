# D2R Audio Mod

独立、无界面的 D2R 音频遥测 Mod 生成/加工工具。它只读取游戏资源或源 Mod，输出一个新 Mod 与 v7 协议清单；不读取 D2RHub 配置、账号、数据库，也不会启用 Mod。

常用示例：

```powershell
# 从本机游戏创建最小 Mod（默认全地图、全部类别）
d2r-audio-mod.exe minimal --game "C:\Program Files (x86)\Diablo II Resurrected"

# 在 jcy Mod 上附加声纹，输出到游戏 mods 目录
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --game "C:\Program Files (x86)\Diablo II Resurrected"

# 只加工符文、钥匙和器官
d2r-audio-mod.exe augment --source "D:\Mods\jcy.mpq" --track runes,keys,organs
```

运行 `d2r-audio-mod.exe help` 查看完整参数。生成完成后，按工具输出的 `-mod ... -txt` 参数自行启用新 Mod。

## 仓库边界

- `src/` 只包含 Mod 的创建、加工和命令行入口。
- `crates/d2r-audio-protocol/` 是生产端使用的版本化音频协议实现。
- `protocol/` 保存生产端与任意接收端都可实现的 v7 格式约定。
- 本仓库不引用 D2RHub 源码或本机的 D2RHub 仓库路径；接收端只需实现同一协议并读取生成 Mod 内的清单。

## 构建与验证

```powershell
cargo test
cargo build --release
```

发布文件为 `target/release/d2r-audio-mod.exe`。仓库不包含生成出的 Mod、游戏资源或构建目录。

# StormLib 技术验证

这是独立的 Windows x64 技术验证工具，尚未接入加工器 CLI 或 D2rHub。
只写入新建测试目录；不会原位转换 Mod，不修改可选输入目录。
测试提取器只接受已知输入清单中的路径，不是可直接处理任意第三方包的生产解压器。

## 运行

需要 Git、CMake、Visual Studio 2022 C++ 工具链和 Rust MSVC 工具链。

```powershell
# 在 d2r-audio-mod 仓库根目录执行
./examples/stormlib-probe/run.ps1

# 可选：只读打包现有目录，再提取到新测试目录并逐字节比较
./examples/stormlib-probe/run.ps1 -SourceDirectory 'C:\Diablo II Resurrected\mods\NullHub\NullHub.mpq'
```

脚本将官方 v9.40 拉到 target 内的唯一目录并校验提交
`6bb1882bd00ddbc3729cac5dac0fda81a61e5514`。
随后静态构建 StormLib、运行 C++ 往返测试、编译运行 Rust FFI 测试。
所有本地写入保留以便检查，不自动删除测试数据；指定 WorkDirectory 时也必须是尚不存在的目录。

## 2026-09-28 验证结果

- StormLib v9.40、MSVC 19.44、Windows SDK 10.0.26100.0、CMake 4.3.2。
- Windows x64 Release 静态库构建通过：BUILD_SHARED_LIBS=OFF、STORM_UNICODE=ON、STORM_USE_BUNDLED_LIBRARIES=ON。
- 中文及空格目录、中文 MPQ 文件名通过。
- Zlib 包：4 文件、165579 字节，往返逐字节相同。
- Bzip2 + MPQ 加密条目：相同的 4 文件全部通过，覆盖空文件、多扇区重复文本和二进制内容。
- 无 listfile 包：4 个条目均枚举为 FileXXXXXXXX.xxx，原始路径无法从此枚举恢复；生产转换必须拒绝这种未解析状态。
- 本机 NullHub 目录重新打包：21270 文件、78907956 字节，解压后全部与只读源目录逐字节一致。
- Rust extern "system" 调用只读打开、读取及关闭成功，modinfo.json 字节完全相同。
- dumpbin 检查 Rust 测试程序不依赖 StormLib.dll；仍依赖系统 DLL 和 VC Runtime，不能宣称完全无运行库依赖。

实测修正：

1. SFileCreateArchive 会强制启用 listfile；缺失清单的测试必须用 SFileCreateArchive2，将 dwFileFlags1 置 0。
2. 深层资源输出会超过传统 MAX_PATH。提取及后续读取校验均需使用 Windows 扩展长度路径，不能只修提取函数。
3. Rust 链接需要显式添加 user32；否则 wsprintfW 出现 LNK2019。StormLib 内部还引入 WININET，已在程序依赖中确认。
4. CMake 未找到系统 ZLIB/BZip2 不导致失败，已显式使用源码内置依赖。

## 尚未覆盖

首次扫描本机 C: 与 D: 两处游戏 mods 未找到单文件 MPQ；NullHub 测试是目录重新打包。随后用户提供的原始 mini.mpq 已完成下述真实样本对比。
本工具未实现生产所需的完整条目覆盖证明、locale/alias 处理、恶意路径全面验证、备份替换事务、故障恢复及 Hub 调度。
未改动游戏文件，没有执行游戏内运行验证。后续正式接入前需要原始 MPQ 样本与上述测试。

## 用户原始 mini.mpq 验证（2026-09-28）

输入：`C:\Users\郭佳燚\Downloads\mini一键退出\mini一键退出\mini.mpq`。
参考：`C:\Users\郭佳燚\Downloads\MPQEditor\Work`。
输出：`D:\pro\d2r-audio-mod\target\mini-original-mpq-compare-v2\mini\mini.mpq`。
逐文件摘要报告：`target/mini-original-mpq-compare-v2/comparison.json`。

- 原包 2819954 字节；读前与读后 SHA-256 相同：`672b13c63a0a01813345c21f25600aea1c474245df8e2ebc13cffaf703b4e9b9`。
- 37811 文件、5272788 字节，与 MPQEditor 参考逐字节及独立 SHA-256 对比一致；missing/extra/changed 均为空。
- 其中 37660 个零字节文件，必须保留，不能作为空内容优化掉。
- 对比包含 `(listfile)`（2985747 字节）；扣除该内部清单后为 37810 个资源文件、2287041 字节。
- modinfo.json 内 name=mini、savepath=mini/；输出按 `mini/mini.mpq/data` 层级生成。
- StormLib 只读打开原包，未使用参考目录的 listfile 补充文件名；参考目录仅用于输出路径白名单及内容比较。

发现一个名称编码差异：包内 `mainmenupanelhd-` 后的字节为 `CE DE BF EC BD A8 B7 BF`，不是有效 UTF-8；GUI 输出为 `mainmenupanelhd-%CE%DE%BFì%BD¨%B7%BF.json`。
对比工具通过反向解码 GUI 名称的 %XX 与 Latin-1 字符，并与 MPQ 原始名称字节精确匹配，建立唯一映射；没有猜测替换中文。这一项也通过内容校验。
该适配仅用于 GUI 对比，依赖参考名称，不代表生产转换已解决名称编码策略。正式转换必须明确非 UTF-8 名称的处理及碰撞规则，并验证游戏行为；不能把“与 GUI 一致”直接等同于“游戏加载完全等价”。

构建后新增调用方式（输出目录必须尚不存在）：

```powershell
stormlib-probe.exe --compare '<原始MPQ>' '<GUI提取目录>' '<新输出目录>'
python examples/stormlib-probe/compare_trees.py '<GUI提取目录>' '<新输出目录>' --archive '<原始MPQ>' --report '<新报告.json>'
```

# MPQ 解压与恢复 CLI

实现位置是 **d2r-audio-mod 加工器**。StormLib 已静态链接在加工器中；Hub 只需调用命令、消费进度和结果，不需要 StormLib DLL，不负责转换事务。

## 调用方式

```powershell
d2r-audio-mod.exe hub-compatibility
d2r-audio-mod.exe capabilities --json
d2r-audio-mod.exe unpack-mpq --source 'D:\Games\D2R\mods\mini\mini.mpq' --events
d2r-audio-mod.exe recover-mpq --mod-directory 'D:\Games\D2R\mods\mini' --events
d2r-audio-mod.exe unpack-mpq --help
d2r-audio-mod.exe unpack-mpq --licenses
```

必须使用绝对路径及标准 `<名称>/<名称>.mpq` 布局；外层名称和包名忽略大小写匹配，保留实际拼写。下载目录若叫“mini一键退出”，先安装到 `mods/mini/mini.mpq`，再调用转换。转换不替用户重新命名 Mod。

`--events` 输出逐行 JSON：progress、completed、error。`--json` 或省略输出选项时成功输出单个结果 JSON。退出码 0 成功，2 失败；失败诊断在 stderr，events 模式的操作错误同时输出结构化 error。两种输出选项不可同时使用。

capabilities 返回 mpq_unpack_v1、mpq_recover_v1；不支持的旧加工器不能执行这一步，但原目录源加工逻辑不受影响。

## 文件布局

```text
mini/
  mini.mpq/                         转换后的源目录
    modinfo.json
    data/...
  back/<UUID>/
    mini.mpq                        原包，内容不变
    files.json                      资源路径、原名称字节、长度、SHA-256
    conversion.json                 最终转换记录
  .d2rhub-mpq.lock                   跨进程互斥文件；存在不等于被占用
```

解压先在 `.d2rhub-unpack-<UUID>/payload` 中完成，校验通过后备份原包并提交目录。提交重命名不覆盖已有目标。活动日志 `.d2rhub-unpack.json` 由加工器维护；调用方不要修改或删除。

`(listfile)`、`(attributes)`、`(signature)` 不进入目录资源树，原包备份仍完整保留这些内容。所有零字节资源均保留。外层目录其他文件不变。不会复制出多余的 `mini/mini.mpq/mini/mini.mpq` 层级。

## 名称与支持范围

- 正常 UTF-8 文件名原样保留。
- 非 UTF-8 路径组件逐字节处理：非 ASCII 字节及 `%` 编码为 `%XX`，不猜测 GBK 或本机代码页。原名称十六进制保存在 files.json；结果 escaped_file_names 明确列出映射。
- 这与某些旧 MPQEditor 版本的混合 Latin-1 转义名称可能不同。样本 mini.mpq 有 1 项此类名称，内容已通过原名称字节映射与 GUI 结果验证。转义名不承诺与包内名称具有相同的游戏引用语义；调用方应展示此映射提示，不能隐藏。
- 缺少可解析原名称而出现 FileXXXXXXXX.xxx 占位项时拒绝转换；不借助 GUI 提取目录、不下载或猜测 listfile。
- 补丁条目、非中性 locale、无法无损表示的条目别名不支持。上限为 100 万有效条目、64 GiB 解压资源；超限在提交前拒绝。
- 拒绝越界路径、Windows 设备名、ADS、大小写文件冲突、文件/目录冲突、链接及重解析点。根目录必须直接有有效 modinfo.json 对象及 data 资源。

## 结果合同

completed.report 的转换结果包含：

```json
{
  "schema_version": 1,
  "status": "converted",
  "mod_name": "mini",
  "mod_directory": "D:\\Games\\D2R\\mods\\mini",
  "mpq_directory": "D:\\Games\\D2R\\mods\\mini\\mini.mpq",
  "backup_path": "D:\\Games\\D2R\\mods\\mini\\back\\<UUID>\\mini.mpq",
  "transaction_id": "<UUID>",
  "archive_sha256": "<sha256>",
  "file_count": 37810,
  "uncompressed_bytes": 2287041,
  "escaped_file_names": []
}
```

status=already_directory：已是目录，仅返回名称和目录位置，不创建新备份。
unpack-mpq 若先恢复已发布的事务，则返回 status=committed 及完整转换报告，保留备份路径和文件名转义映射；后续无活动事务的重复调用仍返回 already_directory。
恢复 status 为 no_transaction、rolled_back 或 committed。
error 包含 code、message、recovery_required；RECOVERY_REQUIRED 必须由调用方阻止继续使用中间态，展示错误路径。

## 中断恢复与调用顺序

1. Hub 或其他调用方检查游戏占用、目标名称与功能参数。
2. 选中压缩源时调用 unpack-mpq，等待子进程退出且收到 completed。
3. 加工器成功转换后，再调用现有 augment，指定新名称和输出父目录。
4. 后续加工失败时保留已转换源和备份，重试直接加工目录源。

加工器普通失败会自动尝试恢复。外部强制终止后，调用方必须等进程完全退出，再调用 recover-mpq；不能沿用已经触发的取消信号继续杀恢复进程。下次 unpack-mpq 也会先恢复。

恢复会检查日志、实际路径和摘要：原包未移动则保留原包；已备份但未发布则恢复原包；目录已发布但未记完成则完整校验后补记提交。冲突或校验失败保留全部材料，不覆盖用户改动。

跨进程锁只协调本加工器命令。游戏进程占用检查仍由 Hub 的现有管理逻辑负责；CLI 会拒绝无法获得源写入保护或无法重命名的情况。不要在游戏使用源 Mod 时执行转换。

## 构建及验证

Windows x64/MSVC 构建依赖 CMake；StormLib v9.40 源码固定在 crates/stormlib-sys/vendor，构建不下载源。运行用户不需要 CMake 或 StormLib.dll；VC Runtime 仍按现有发行方式处理。发布时携带 crates/stormlib-sys/THIRD_PARTY_NOTICES.md，二进制中也嵌有该声明，可通过 --licenses 查看。

```powershell
cargo test mpq
cargo test
cargo build --release
```

已验证：

- 原始 mini.mpq 的副本转换，37810 资源、37660 零字节文件、2287041 字节；与 GUI 结果按原名称映射逐文件 SHA-256 一致，备份摘要与原包一致。
- 同一路径重复调用返回 already_directory；无事务恢复返回 no_transaction。
- 原 augment 加工生成独立 MiniMpqTest，局内房间工具生成成功；加工后源树与备份再次校验不变。
- 测试注入覆盖 extracting、prepared、backup_pending、原包移动后、publish_pending、目录发布后、committed 各边界；恢复幂等。
- 分别在解压过程中和原包移动到备份后强制结束真实 CLI，再单独运行 recover-mpq，两种情况均成功恢复原文件及原摘要。
- 全套测试：45 通过、7 忽略、0 失败，包含发布后中断重试保留备份与转义映射的回归；此前 Windows x64 Release 构建通过。

原始下载 MPQ 和 GUI 提取目录均未改动。未进行游戏内加载验收；Hub 已接入独立解压入口、任务进度及中断恢复。

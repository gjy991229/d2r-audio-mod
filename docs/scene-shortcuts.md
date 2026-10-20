# Scene shortcuts — current r7 and development history

Current recipe: `scene_shortcuts` r7, opt-in via `--features scene-shortcuts`.
It embeds ten reference layouts and the Act 3/4 waypoint mapping used by the
paired Hub validator. The help key starts the restart chain; the entry HUD
attempts the arrival waypoint action after loading. Overwritten layouts and
the original waypoint table are backed up in the generated Mod. It does not
apply Hub room naming rules. Restart or create a game at an A3, Travincal, or
A4 waypoint. This module uses native game UI messages, without image recognition
or OCR.

The embedded `src/generator/scene_templates/mymod_exact/` files are byte-exact
reference snapshots. Git attributes preserve their original line endings and
whitespace; do not autoformat them, since that changes the validator digests.

The sections below describe earlier iterations and their corrections; r1
timer values and removed toolbar entries are not the current recipe.

## Historical r1

Opt-in feature `scene_shortcuts`, CLI `--features scene-shortcuts`.
The default `all` remains audio + rooms; this module is never implicitly selected.

This reproduces MYMOD's waypoint/restart chain from the **current scene**:
open WaypointsPanelExpansion, open a native-derived waypoint controller at
100ms, request exit + next Hell game at 200ms, unload the entry at 300ms.
The helper retains MYMOD's tabCount=4, defaultTab=2 and
Waypoints:SelectWaypoint control-event wiring. Those values are not level IDs.

The user reports MYMOD returning from Travincal to the next game's Travincal
waypoint after loading. The generated implementation has not yet been verified
in game. Mephisto/Durance and other scenes need separate testing; no third-floor
Mephisto destination is invented.

Both a top HUD button and the existing game Help key trigger the chain. The
HD help layout is replaced, with its original document preserved at
`<name>.mpq/d2rhub-scene-shortcuts-original-help.json`. Keyboard bindings are not
changed. The feature does not modify room automation's keyboard gateways and
does not require audio or room tools. It uses CharacterSelect:LoadCharacter:2,
not named-room creation or the Hub's room-number sequence.

The implementation is isolated in `src/generator/scene_shortcuts.rs`. Minimal,
augment and audio-reuse branches install it and retain its independent feature
metadata. Additive processing preserves the original help backup; unknown
future recipes are rejected instead of downgraded. Hub validates the generated
chain and required files before accepting the output.

Only HD layouts are generated. Existing normal/help functionality is replaced
for this explicitly selected module; legacy graphics and controller behavior
are not certified. In-game scene/waypoint availability and server timing still
apply, and the module does not claim an acknowledged scene transition.


## r2 correction (2026-10-19)

The user observed r1 restarting into Act IV, not Travincal. The original
WPautuexitonekey belongs to the MYMOD help variant labeled return-to-A4.
r2 uses tabCount=3 from CALL/WP-t-autuexitonekey (Travincal), keeping
defaultTab=2. Actual landing still needs an in-game test near an interactable
waypoint; this is not a verified arbitrary-scene restart.
The toolbar now launches immediately as the first HUDWarnings child, before
JCY closes that panel at 8ms. Exact r1 fingerprints can upgrade to r2.


## r3：按已启用 MYMOD 的完整两段链修正

前段 H/按钮：原生小站面板 → 100ms 选择 tabCount=4/defaultTab=2 → 200ms 退出并重建地狱 → 300ms 清理。后段每次进房：HUD 立即打开独立 Entry 面板，1 秒后执行 Arrival，后者立即打开小站面板、100ms 用 tabCount=3/defaultTab=2 选择崔凡克、300ms 清理。Entry 在1.4秒自行卸载，不受 JCY 在8ms关闭 HUDWarnings 影响。传送清理不再提前卸载前段重建控制器。

保持 MYMOD 的行为：每次进房都会尝试传送，并非只在按 H 重建后执行，也不是保留任意当前地图。只有崔凡克参考链；实际游戏落点仍待用户测试。安装时去除源 MYMOD 的同用途 OnekeyTravincal HUD 启动项，避免重复执行。r1/r2 精确指纹可升级至 r3，未来版本拒绝降级。Hub 校验两套传送参数、独立进房控制器及全部时序。此次不修改用户的 jcy-x，由用户自行加工。


## r4：仅保留帮助快捷键入口

删除顶部场景重开按钮及启动器，升级旧版时移除本模块的旧工具栏文件。独立进房控制器仍在启动后1秒启动传送链、1.4秒卸载自身；1.4秒是兼容实现的生命周期安排，不是MYMOD参数或传送成功确认，也不代表按H后固定等待1.4秒。r1/r2/r3可升级，实际落点仍需测试。


## r5：直接 HUD 触发，移除独立进房控制器

用户确认：当前启用的 MYMOD 在崔凡克或 A3 主城能看见小站的位置按 H，可以直接加载下一房间并到达崔凡克；r4 成品却停在 A4。静态文件不能确定失败发生在哪一步。r5 去掉自行添加的 SceneEntry Panel 和1.4秒卸载，恢复参考中的 HUDWarnings 1秒直接启动 Arrival。JCY 的8ms HUD关闭动作延至1秒，传送启动放在关闭前；其余 HUD 子项保留。这是明确的兼容调整，不声称已实机验证。

两套 waypoint 控件改用当前 MYMOD 的 WPautuexitonekey/WP-t-autuexitonekey 文件快照，随加工器编译内置；仅重命名面板/引用和清理计时器，不再从原版控件拼装。模板比较测试验证非计时器字段一致。旧r1–r4作为升级来源按原配方校验，新产物要求r5，并拒绝旧独立控制器、顶部按钮或1秒前关闭HUD。用户自行更新jcy-x，本次未修改游戏目录。


## r6：原始链路文件照搬，HUD合并单列

已取消自行生成的D2RHubScene控制器。内置当前用户已确认可用MYMOD的10个原始文件（帮助页高清/原版、原版小站布局、CALL目录辅助面板及QuickHellGamePanel），原字节写入成品，不重命名面板、控件或消息。SHA-256清单在Hub resources/scene-shortcuts-v6.json；新增校验按此清单检查原样文件。覆盖前的源布局保存在d2rhub-scene-original，已有帮助页备份保留。

HUD不能整体覆盖，否则会引用源Mod不存在的其他功能。因此保留源HUD内容，移除旧D2RHubScene启动器，末尾放入MYMOD原来的1秒UnloadPanel:HUDWarnings、1秒OpenPanel:CALL/OnekeyTravincal两个计时器（顺序及重复名称保留）。JCY的OpenPanel:JcyHudWarnings由1ms改为1秒，这是兼容合并，不是原文件字节照搬。每次进入游戏都会尝试传送，实际游戏行为尚待用户验证，不能从静态比较宣称修复成功。此次未改动jcy-x。

验证：r1–r5来源校验、r6新产物校验、JCY+拾取组合、文件篡改与HUD冲突拒绝、10个文件与本机有效MYMOD逐字节比较均通过。


## r7：补齐配置器强制联动的 actinfo 小站映射

重新检查MYMOD配置器的PyInstaller归档（仅解压数据、未执行配置器）找到实际实现文档字符串：_apply_onekey_travincal_state 联动按钮、HUD文字、actinfo目标与“四幕传送点修改”；一键崔凡克强制勾选该修改，先复制参考actinfo再修改第5行M列（act4/waypoint4）为Act 3 - Travincal。先前r1–r6只关注UI调用，未覆盖该依赖，是明确遗漏。

实际比较：MYMOD act3 的waypoint2/3为Act 4 - Town/Act 3 - Travincal，jcy-x对应仍是Act 3 - Jungle 1/Act 3 - Jungle 2；act4若干额外槽位也不同。jcy-x虽已有act4/waypoint4=Travincal，单这一格并不代表完整参考列表相同。r7按当前可用MYMOD同步act3/act4全部9个waypoint字段，保留其他幕、其他列及源文件换行风格；保留覆盖前actinfo备份。

原版CASC与MYMOD的小站objects关键字段一致；MYMOD与jcy-x的levels逐字段比较只见声纹SoundEnv差异，因此本次不覆盖objects或levels。10个UI原始文件与r6相同，HUD接入也不新增延时猜测。Hub r7校验增加actinfo映射检查，旧版来源仍按旧配方接受。规则篡改测试必须能拒绝恢复为旧第三幕映射的产物。静态修正补齐了真实配置依赖，最终游戏落点仍待实测。

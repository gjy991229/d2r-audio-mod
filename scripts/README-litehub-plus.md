# LiteHubPlus personal builder

This development workflow builds a separate, controller-free Mod from a verified
`main` baseline, the current installation's CASC, and an installed JCY `.mpq`.
No JCY program is executed and no reference directory is modified. Third-party
assets remain local inputs; this is not a distributable bundled release recipe.

1. Build the development CASC exporter with `cargo build --release --example litehub-casc-export`.
2. Use a verified downloaded LiteHub product matching the installed game as the baseline. The processor no longer generates LiteHub, BoHub or NullHub.
3. Run Python with Pillow installed:

```powershell
python scripts/build_litehub_plus.py --game 'C:\Diablo II Resurrected' --baseline 'D:\build\baselines\LiteHub' --jcy 'C:\Diablo II Resurrected\mods\jcy\jcy.mpq' --work 'D:\build\work' --output 'D:\build\output' --exporter target/release/examples/litehub-casc-export.exe
```

The builder refuses to replace an existing `output/LiteHubPlus`. Failed staging
directories remain for diagnosis. Copy the final `LiteHubPlus` directory into
the matching game's `mods` directory. Use `-mod LiteHubPlus -txt -assettestmode 1`.
Existing account configuration and savepath are preserved.

The result includes 26–33 rune and key/organ beams, BO/BC/Shout state-end audio,
JCY entrance markers and eight Tower/Durance directional presets, plus new
code-generated inventory/stash reference grids. Current native UI interactions,
capacity and special stash tabs are preserved. It does not include telemetry or
room automation by default.

Dependencies resolve compiled model LODs, split RGB/alpha textures and combined
animation bundles. `enhancement-manifest.json` records input hashes, resolved
dependencies and all output hashes. It deliberately does not impersonate an
unchanged stock `generation-manifest.json`; the original baseline manifest is
retained under its own name. Hub can scan the result as an ordinary Mod.

File integrity, targeted table changes, unchanged UI interactions and resource
references are checked during construction. `runtime_verified` remains false:
game rendering, sound scope, refresh/death/scene-change behavior, navigation
exceptions, lowend rendering and frame time still require a gameplay pass.

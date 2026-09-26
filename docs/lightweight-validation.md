# v1.4.0-beta.1 local generation record

Branch: `codex/lightweight-mod-generator`. Local game build: `93854`.
Settings: texture maximum side 4, sprite divisor 2, all asset categories.

| Output | Empty overrides | Rewritten JSON/frontend | Small textures | Resized sprites | Generated resource content |
| --- | ---: | ---: | ---: | ---: | ---: |
| D2RLight-main-b1 | 4,972 | 2,666 | 4,613 | 21 | 15.68 MiB |
| D2RLight-filler-b1 | 19,646 | 268 | 1,425 | 20 | 19.45 MiB |
| D2RLight-min-b1 | 21,163 | 21 | 0 | 64 | 54.57 MiB |

Sizes exclude README/manifest/metadata and filesystem overhead. They are not
RAM or VRAM measurements. Min carries more nonempty sprite targets, explaining
its larger generated package despite more aggressive world-resource blocking.

No unsupported native texture/sprite formats remained in these three runs.
Main reported 456 missing original paths (455 texture paths and the mod-only
default biome); filler reported 19 (18 texture paths plus that biome). They
were omitted, not replaced with borrowed mod data. Some original definitions
already matched the structural selection and required no generated override.
The complete per-file results are stored in each generated manifest.

Validation performed:

- 42 unit/regression checks passed, including all 9 new lightweight checks.
- Three pre-existing room-tool tests were excluded: they require a game
  baseline but their fixtures supply none; the same failures were previously
  reproduced on the unchanged baseline. Four opt-in game-data tests remain
  ignored in the normal suite.
- Generated nonempty JSON/frontend files were independently parsed; every
  generated texture's mip offsets, lengths, format, dimensions and total size
  were checked; sprite payloads, frame counts and supported spacing were checked.
- All generated nonempty texture dimensions had maximum side <= 4.
- The original audio processor successfully augmented the generated main
  profile with rune telemetry and Countess-route area markers in a separate
  test output directory.
- Release build succeeded. Standalone EXE: 2,384,384 bytes (2.27 MiB), versus
  1,709,568 bytes for the prior v1.3.4 build: +674,816 bytes (~0.64 MiB).

The game was **not launched**. Rendering, UI scale/hit targets, interactions,
particle appearance and memory savings are not validated by these file checks.
The Windows GUI was compiled but not interactively exercised. Use the standalone
preview executable; the installed D2RHub sidecar was not replaced.

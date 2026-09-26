# v1.4.0-beta.3 validation

Default texture maximum side is 4. Default sprite divisor is 2, applied after
reference masks, native lowend selection and frame selection. Explicit scale 0
retains beta.2 reference dimensions, and scale 1 retains original nonempty sprites.

| Profile | Generated resource MiB |
| --- | ---: |
| main | 15.61 |
| filler | 14.10 |
| min | 18.98 |

47 regression checks passed, including a combined frame-selection/mask/resize
case. The same three pre-existing room-tool fixture failures were excluded;
four opt-in data tests remain ignored. All generated JSON was parsed, all texture
mip ranges and maximum-side limits checked. All 103 sprites were independently
checked against beta.2: retained frames, reduced geometry, exact payload length,
and every output alpha value against the corresponding source area average.

Test directories: `C:\Diablo II Resurrected\mods\D2RLight-{main,filler,min}-b3`.
Preview: `target/lightweight-preview-b3/d2r-audio-mod.exe`.
Previous builds and the stable D2RHub sidecar were preserved. Game rendering,
UI layout and memory usage still require in-game testing. No game was launched.
Beta.2 exclusions (corrupt reference textures, custom artwork and nonempty
particle/legacy assets) remain unchanged.

---

# v1.4.0-beta.2 local generation record

Branch: `codex/lightweight-mod-generator`; local game build `93854`.
Default settings: per-reference texture dimensions and sprite operations, all categories.
Beta.1's uniform 2x sprite reduction has been withdrawn.

| Profile | Empty | JSON/frontend | Texture | Sprite | Resource MiB | ZIP MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| main | 4973 | 2666 | 4508 | 20 | 56.32 | 6.90 |
| filler | 19653 | 268 | 1322 | 19 | 54.34 | 8.03 |
| min | 21163 | 21 | 0 | 64 | 75.07 | 20.15 |

Resource sizes exclude metadata and filesystem allocation; ZIPs include manifests.
These are not RAM/VRAM measurements. Keeping the reference canvas makes raw
RGBA larger than beta.1's incorrect blanket downscale, even when mostly clear.
Min uses 52 native lowend substitutions plus 12 individually processed sprites.
Filler's globe becomes one frame (490,040 bytes), discarding the reference's
unused tail instead of restoring a 46-frame native animation.

46 unit/regression checks passed; 4 opt-in data tests were ignored. Three known
pre-existing room-tool baseline-fixture failures were excluded (same exclusions
as beta.1); this is not a claim that the entire unfiltered suite passes.
Independent output verification checked every generated target against its recipe,
parsed all nonempty JSON/frontend, validated every texture dimension/mip/range,
and checked sprite dimensions/frame counts/exact payload sizes. All non-lowend
sprite pixels transparent in the reference remain transparent in the output.
No generated asset path was added outside the source target list (except mod
metadata/build marker). Release EXE is 3,109,376 bytes (2.97 MiB).

Explicit limitations: three main fallen-hair reference texture headers are corrupt
and excluded with manifest reasons. Custom painted RGB colors, alpha gradients,
nonempty particles and nonempty legacy image/table assets are not reconstructed;
the game supplies original resources for documented exclusions. Missing mod-only
paths are not invented. Output is not visually or behaviourally identical to lowHD.

The game was not launched. The GUI compiled but was not interactively exercised.
The stable D2RHub sidecar and previous MOD folders were not replaced. Test outputs:
`C:\Diablo II Resurrected\mods\D2RLight-{main,filler,min}-b2`.
Standalone preview: `target/lightweight-preview-b2/d2r-audio-mod.exe`.

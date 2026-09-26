# Lightweight resource generator — format and provenance notes

This implementation consumes files from the user's local CASC. It does not
bundle textures, sprite pixels, particle binaries, complete game JSON files,
or a third-party converter. The only new dependency is `flate2`, used to load
compressed target/selection recipes (already present transitively).

## Texture mip selection

The format layout was cross-checked against the [texture template](https://github.com/CucFlavius/Zee-010-Templates/blob/main/DiabloIIResurrected_Texture.bt),
the [MIT-licensed TextureReader](https://github.com/D2R-Reimagined/reimagined-level-editor/blob/master/src/D2RLevel.Assets/TextureReader.cs),
and local native/low-resolution file headers. Implementation is independent.

For observed 2D textures, the 36-byte header is followed by eight-byte mip
records. Each mip offset is relative to the offset field itself, not the
file start. The byte at offset 7 mirrors the mip count at offset 28.
Formats 31 (RGBA), 57/58 (BC1), 61/62 (BC3), and 63 (BC4) are validated by
their expected payload sizes. Unknown flags, formats, depth, ranges, or
trailing data cause a native fallback recorded in the manifest.

The generator selects the first native mip whose longest side is no greater
than the requested limit and retains its remaining mip chain. It rebuilds
dimensions, counts and relative offsets; compressed payload bytes are not
decoded or recompressed. This uses Blizzard's original small mip, including
its treatment of normal/material channels. RGBA files without a suitable
mip can be area-averaged and supplied with a new mip chain. Compressed files
without a sufficiently small mip remain native rather than being guessed.

## SpA1 sprites

Header layout references: the pinned casc-core SpA1 notes and the public
[SpriteEdit file writer](https://github.com/eezstreet/D2RModding-SpriteEdit).
SpriteEdit source is GPL; no converter implementation is incorporated here.
Only format facts were consulted. The reducer is independently implemented.

This first version supports RGBA format 31 with a 40-byte header, horizontal
frame strips and observed zero-, one- or two-pixel spacing. Both padding on
every cell and gutters only between cells are supported. Frame-width metadata,
atlas width/height, payload length and channel count are regenerated. Frames
are independently area-averaged with alpha-weighted RGB to avoid cross-frame
bleeding and dark transparent edges. Native sprites with zeroed size/channel
header fields are accepted when actual RGBA payload dimensions agree exactly.

Resizing preserves frame count and strip structure, **not a promise of the
same rendered UI size**. D2R's consumer may use pixel dimensions for layout.
Users can choose sprite-scale=1 to retain native nonempty sprites. Lowend
counterparts follow the same selected divisor. Game-side validation remains
necessary; successful file generation does not prove rendering correctness.

## JSON selection recipes

The lowHD directories supplied locally by the user provide target paths and
structural selection rules. Native CASC is the only source of output values.
Rules can remove object keys, select existing named/identified array members,
clear strings/lists, disable flags, set numeric fields to zero, or redirect a
mapping to another native entry. Unknown mod-only entities and arbitrary
replacement strings/numbers are never transplanted. New lowHD-only paths that
do not exist in native CASC are reported and omitted. This is not a byte-for-byte
or behaviourally identical rebuild of lowHD.

Most nonempty custom UI layouts fall back to native UI. Selected layouts keep
structural culling but use native surviving fields. Mod-only timers and custom
party panels are not added. Empty overrides retain the reference profile's
blocking intent. Nonempty particles, DDS/DC6 and table modifications are not
regenerated in this version; the manifest explicitly identifies native fallbacks.

## Safety and validation

Every recipe path is validated and duplicate paths are rejected. Generation
writes only into a new UUID staging directory under the selected output parent.
Existing output names receive a numeric suffix. Fatal read/write failures remove
only that owned transaction; unsupported formats are explicit per-file fallbacks.
The tool never starts the game, changes active mods or rewrites original data.
Unit checks cover mip offsets/payloads, RGBA averaging, frame boundaries/gutters,
structural pruning, bundled profiles, traversal rejection and transaction cleanup.

Reference project: [lowHD — celloboy126 / evilbelgian](https://www.nexusmods.com/diablo2resurrected/mods/1054).
Attribution does not relicense its material. The project's MIT license applies
to our code; it does not grant rights to Blizzard's generated asset content.

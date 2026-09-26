# Current beta5

Current independent rules are documented in [independent rules](lightweight-independent-rules.md).
The reference-specific rules below describe retired beta2–beta4 implementations;
their recipe formats, JSON deltas, masks and image geometry are no longer shipped
or accepted by beta5. The native texture/sprite byte layouts remain relevant.

---

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
expected payload sizes. With texture-size=0, dimensions, format and mip count follow each
reference target. The beta.3 default is maximum side 4, using native mips. Matching native mip payloads are retained directly; otherwise
small native mips are decoded, resized independently by channel, and encoded
with an independently implemented BC codec. Explicit texture-size overrides
remain available. Unknown conversion strategies stop generation.

## Reference sprite operations (beta.3)

RGBA `SpA1` and `SPa1` headers are supported. Default divisor is 2, applied AFTER reference operations.
Each recipe records source fingerprint, geometry, frame selection and rectangular
transparent/black operations. Original game pixels supply surviving visible areas.
Where reference geometry matches a native `.lowend.sprite`, the native lowend
variant is used directly. Other sprites preserve reference dimensions, blank
regions and selected frames. Unused reference trailing bytes are discarded. Each resulting frame is then
independently reduced with alpha-weighted RGB; no discarded background or frame
is restored by the resize. sprite-scale=0 retains reference dimensions.
Arbitrary painted colors are not imported; differences are reported in the manifest.
This preserves structural operations, not identical artwork or all alpha gradients.

Source fingerprint changes stop conversion so old masks cannot silently be applied
to a changed game image. Native overrides (`sprite-scale=1`) are explicit user choices.
RGBA byte size still depends on canvas dimensions even when most pixels are clear.
Clearing background pixels alone does not establish a VRAM reduction.

## JSON recipes (beta.4)

Non-UI JSON/frontend targets use fingerprinted native input plus recursive deltas.
Changed strings, nonzero numbers, booleans, nulls and dependency arrays are now
preserved, including fake.texture, null skeletons and default-biome redirects.
Only differences are encoded; identical values come from the installed game.
Object removals/edits and array edits describe structural changes. Recipes contain
reference parameter values, not merely target paths; they are not independent of
the reference author's configuration. No reference image payloads are embedded.
The missing default biome is derived from native act1_outdoors.json and edited
to the reference definition. Import verifies reconstruction against the reference.
Runtime rejects changed native fingerprints instead of misapplying positional edits.

UI paths retain the prior policy: selected layouts receive structural culling;
other custom UI layouts fall back to the game. Mod-only timers/party panels are
not added. Empty overrides retain the reference blocking intent. Nonempty
particles, DDS/DC6 and table changes remain explicit exclusions.

## Safety and validation

Every recipe path is validated and duplicate paths are rejected. Generation
writes only into a new UUID staging directory under the selected output parent.
Existing output names receive a numeric suffix. Fatal read/write failures remove
only that owned transaction. Unconfirmed conversion strategies stop generation.
Three main reference hair textures have corrupt space-filled dimension fields;
they are explicitly excluded and reported, without guessing a replacement.
The tool never starts the game, changes active mods or rewrites original data.
Unit checks cover mip offsets/payloads, RGBA averaging, frame boundaries/gutters,
structural pruning, bundled profiles, traversal rejection and transaction cleanup.

Reference project: [lowHD — celloboy126 / evilbelgian](https://www.nexusmods.com/diablo2resurrected/mods/1054).
Attribution does not relicense its material. The project's MIT license applies
to our code; it does not grant rights to Blizzard's generated asset content.

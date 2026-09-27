# Resource formats

## Textures

Observed 2D textures use a 36-byte header and eight-byte mip records. Each record's
offset is relative to its offset field. Formats 31 (RGBA), 57/58 (BC1), 61/62 (BC3)
and 63 (BC4) are validated using their expected payload sizes. Native compressed
mips are selected without re-encoding. RGBA reduction averages channels separately;
texture alpha can encode material data rather than transparency.

## Sprites

Supported SpA1/SPa1 RGBA sprites use a 40-byte header. Dimensions, frame count,
logical frame width and frame gutters are checked before copying pixel rows.
Frame selection operates on original game pixels. There are no embedded pixel
masks. Blank pixels still occupy space in an uncompressed RGBA atlas.
Changing image resolution is distinct from changing its on-screen drawing size.

## JSON and text

JSON/JSON5 input is parsed into structured values. The recipe applies validated
set/remove operations and verifies the serialized result. Text edits preserve
UTF-8 boundaries. Product metadata and optional instructions use literal text.
Image and particle payloads cannot be stored in text rules.

## Version and output

The data version file is read directly from native CASC. All other rule outputs
are checked against SHA-256 digests. Generation writes to a fresh staging folder,
checks the complete file set and publishes only after successful verification.

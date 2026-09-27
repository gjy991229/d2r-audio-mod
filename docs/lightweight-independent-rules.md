# Lightweight generation rules

The active generator uses embedded recipes and native game CASC. Recipe entries
specify empty overrides, native file selection, texture mip limits, sprite frame
geometry, JSON edits, text edits and product metadata.

The profiles are main (LiteHub), filler (BoHub) and min (NullHub). Their target sets
are fixed. Only explicitly listed files are emitted. Runtime does not scan an
existing mod to decide new targets.

Current behavior:
- Low-resolution sprite selection and frame processing are retained.
- Cursor, map and icon sizes follow the current recipe; there is no separate
  display-size correction or automatic high-resolution substitution.
- Pause layouts retain manual Save and Exit without automatic exit timers.
- Required skeleton components and their configured references are preserved.
- Data version bytes come directly from the game used for generation.
- Output names default to LiteHub, BoHub and NullHub; collisions receive suffixes.

JSON edits are verified against output digests. Native input changes fail clearly;
new game versions may require updated rules. An updated data version alone does
not establish compatibility of changed game assets.

See [recipe format](../resources/lightweight/b13/README.md) and
[validation](lightweight-validation.md).

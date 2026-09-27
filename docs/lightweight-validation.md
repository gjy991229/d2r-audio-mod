# Validation

The review build passed 16 relevant tests, covering texture mips, sprite geometry,
JSON edits, UTF-8 text edits, input/output digest failures, data version passthrough,
required version entries, neutral product metadata and removal of auto-exit timers.
The recipe-authoring test is run separately when rebuilding embedded recipes.

Complete output comparisons verified all three profiles. The branding cleanup
changed modinfo.json and optional settings instructions only; game data files were
unchanged. Current installed game data version was 93854 at verification time.

Standalone generation was verified using an input directory containing only
.build.info and a junction to the native Data directory, with no mods directory.
The executable ran from a separate preview folder without external recipe files.

Verification distinguishes file equivalence from runtime behavior. Compare game
memory using the same character, graphics settings, route and settling time,
starting a fresh client for each run. Static checks do not measure runtime memory.

Current display behavior keeps low-resolution cursor/map assets. No display-scale
fix is enabled. Prior high-resolution substitutions are not part of current rules.

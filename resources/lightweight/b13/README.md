# Embedded generation recipes

The three gzip JSON files are compiled into the executable with include_bytes!.
Runtime requires only the executable and native game CASC.

Each entry records a target path, generation action and verification data:
- Empty override.
- Native file selection with an input digest.
- Texture mip selection or sprite frame geometry.
- JSON/text edits applied to native input.
- Literal text for product metadata and optional instructions.
- Current game data version passthrough.

Image, texture, particle and DC6 payloads are read from the game. Text rules are
restricted to JSON/frontend/txt paths. File paths, required metadata and output
file sets are validated. Input changes or output digest mismatches stop publication.
The data version entry must read data/global/dataversionbuild.txt from CASC and
preserve its bytes. Product metadata contains only the product name and savepath.

The development-only freeze_b12_recipes test compiles a verified baseline into
recipes. Set D2R_FREEZE_GAME to the local game directory containing its baseline
outputs when maintaining rules. Recompile the executable after updating recipes.
This authoring test is excluded from release executables.

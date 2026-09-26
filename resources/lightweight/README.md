# Built-in lightweight recipes

`min.json.gz`, `filler.json.gz`, and `main.json.gz` contain paths, action types,
and structural JSON selection rules. They contain no sprite/texture/audio
payloads or complete replacement game definitions. Source target selection:
lowHD by celloboy126 / evilbelgian, from the user's local reference packages.

Generation reads the corresponding original assets from the user's installed
D2R game. The reference lowHD directory is not needed at runtime. Rules are
intentionally not advertised as an independent recreation of the source's
complete behaviour or as a grant of rights to its contents.

To rebuild a recipe locally, use a fresh output filename:

```powershell
d2r-audio-mod lightweight-import --source "C:\path\lowHDmain.mpq" --profile main --output main-new.json.gz
```

Review the imported recipe before replacing a bundled file. This developer
command never copies reference asset payloads and refuses to overwrite its output.
See [format notes](../../docs/lightweight-formats.md) for supported operations
and limitations.

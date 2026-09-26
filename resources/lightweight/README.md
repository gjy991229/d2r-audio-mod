# Independent native-data processing scopes (beta.5)

The compressed files contain only target paths, abstract actions and exclusion
reasons. Historical target boundaries and empty-override intent originate from
the local lowHD reference packages, retained to avoid scope expansion. These
boundaries are NOT claimed as independently discovered.

No reference JSON values, IDs, scalar deltas, sprite masks, dimensions or frame
matches remain. Processing is defined in src/lightweight/native_policy.rs and
assets.rs using installed game assets only. The legacy reference-reconstruction
recipe formats are rejected. Scope import reads file metadata and BOM markers;
it no longer imports JSON or image contents.

See docs/lightweight-independent-rules.md for exact rules and limitations.

# Room tools r33 and double-Esc r4

The bundled processor retains the r33 native join flow. Double-Esc r4 uses
`esc-next-game-v4;window_ms=500;pause_timeout=1;hud_cleanup=0`.

Both the standalone Esc installer and the room-tools installer remove the
legacy named `D2RHubCloseEscArm` timer from HUDWarnings without reinserting it.
Unrelated HUD children survive, including repeated generation. Cleanup in
pause and transition panels and the 500ms receiver window remain unchanged.

Hub validates generated r4 layouts against this absence requirement. Existing
Mods must be regenerated and the game restarted to load the changed layouts.
Processor unit tests and the Hub clean-rebuild integration test cover this
contract; game interaction still requires in-game verification.

# Room tools r32

Restore the original transition order for both in-game create and join:
open pause at 10 ms, then queue exit, native submission and controller close
at 50 ms in that child order. This replaces r31's 60 ms submission without
self-close. It is a client message order, not a server exit acknowledgement.

Double-Esc remains r3: the persistent HUD cleanup timer stays removed.
Audio telemetry is unchanged. Regenerate the Mod and restart the game to
load the new layout; changing Hub input timings cannot modify an existing Mod.

D2rHub separately supports bounded follower Enter repeats after initial
submission. These do not change the Mod's timer deadlines.

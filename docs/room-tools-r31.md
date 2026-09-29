# Room tools r31 and double-Esc r3

## Create and join transitions

Both in-game forms now open pause at 10 ms, request exit at 50 ms and
submit at 60 ms from confirmation. Neither submission controller schedules
its own close. Previously r30 submitted create at 100 ms and join at 550 ms,
then closed their controllers at 150 ms and 600 ms respectively.

The 10 ms exit-to-submit gap is a tested timing adjustment, not a server exit
acknowledgement. The user reported following/joining worked with the BoHub-based
`testbo` trial. Repeated operations and other network conditions still require
runtime testing. Quick recreate retains its existing 50 ms exit/load ordering.

## Double-Esc and persistent HUDs

The processor previously injected `D2RHubCloseEscArm` into `HUDWarnings`,
closing the Esc receiver after 1 ms. This conflicted with the receiver on
the tested BoHub output. Removing only that HUD timer restored double-Esc
according to the user's in-game test.

The compared JCY output has identical pause, Esc receiver and quick-recreate
layouts, but closes native `HUDWarnings` at 8 ms and opens its own HUD.
That difference is consistent with JCY avoiding the conflict; the engine's
precise timer/reactivation behavior has not been established.

Both room-tool and standalone Esc installation remove the old named HUD timer
without adding it again. Pause actions, transition controllers and the 500 ms
receiver/pause timeouts retain their cleanup. Existing source HUD content is
preserved. No native restart button is required by this message chain.

## Compatibility and verification

- Room feature: r31, fingerprint `room-tools-v31`.
- Esc feature: r3, fingerprint
  `esc-next-game-v3;window_ms=500;pause_timeout=1;hud_cleanup=0`.
- D2rHub must accept prior room recipes and Esc r2 as upgrade sources while
  checking new outputs against the new contracts.
- Regression tests cover 60 ms submission, no self-close, legacy layout
  validation, removal of old HUD cleanup, unrelated HUD preservation,
  repeated generation, retained pause timeout and Esc metadata upgrades.
- These changes do not alter audio telemetry.

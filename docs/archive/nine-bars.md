# Nine bars sound meter (archived)

Nine bars was the default sound meter on the Me and Others buttons from 0.2.0-alpha.10 to 0.2.0-alpha.12. John
picked Line wave as the only style on 2026-11-25 ("The waveform option is much more appealing"), so 0.2.0-alpha.13
removed Nine bars from the app, along with the **Sound meter** setting and the `WaveformStyle` admin key.

## Where the code is

The last version is in tag `v0.2.0-alpha.12`:

- `app/ui/src/lib/waveform.ts`: `BARS`, `SLOTS`, `REST`, `SHAPE`, `SLOT`, `SAMPLE_MS`, `pushLevel`, `waveform`,
  `flat`, `settled`, the bar `Meter` (`hist`, `acc`, `bars`) and `barRects`, which places bars and gaps on whole
  device pixels so the spacing is even at any display scale.
- `app/ui/src/lib/waveform.test.ts`: its tests, including the settle test for a silence that repeats the same 0.
- `app/ui/src/lib/SourceButton.svelte`: the bars branch of `draw()`.
- `app/ui/src/lib/Settings.svelte`, `prefs.svelte.ts`, `managed.ts`: the setting and its admin lock.

To look at it: `git show v0.2.0-alpha.12:app/ui/src/lib/waveform.ts`.

## Lessons kept in the line meter

- The window hears a level only when it changes, so a meter must keep the newest level and run its own frames until
  it is still; otherwise a silence that repeats 0 freezes it mid-swing.
- Draw on a canvas at whole device pixels; CSS gaps of 1.5 px land between screen pixels and look uneven.

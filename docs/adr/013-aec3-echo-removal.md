# ADR-013: WebRTC AEC3 on the microphone, and a loopback clock that keeps time

Status: **accepted** (alpha). Supersedes the software canceller part of ADR-004.

## Context
A work laptop on speakers transcribed a YouTube video as "Me". Two causes were found in the code:

1. **Layers 1 and 2 of SPEC 4.2 were never in the live path.** `NlmsCanceller` existed but nothing called it, so only
   transcript dedupe (layer 3) ran.
2. **Dedupe compared lines on different clocks.** Each stream is timestamped by counting its own samples. WASAPI
   loopback delivers no packets while nothing is playing, so the system stream's clock fell behind the microphone's
   (about 50 s in the report: Me at 1:28, Others at 0:36 for the same audio). Dedupe looks for Others lines within
   plus or minus 2 s, so it never matched.

## Options weighed
- **Denoise the mic (RNNoise, DTLN, Windows noise suppression).** Does not help: bleed is clean speech, and a
  denoiser keeps speech. Also changes audio the transcription model should hear untouched.
- **Windows communications-mode AEC** (`IAcousticEchoCancellationControl`, SPEC layer 1). Good when the driver
  supports it, but depends on the audio driver and Windows build, cannot be tested on the Linux host, and gives no
  signal about whether it worked. Kept as a later optional layer.
- **Speaker embeddings** ("is this my voice?"). Needs an extra model, enrollment, and fails on the user's own cold or
  a second person in the room. Not needed once echo is removed. Revisit if needed.
- **Energy or correlation gating only.** Cheap but crude: it mutes the user when they talk over playback.
- **Speex MDF / NLMS.** Older, weaker on delay drift and nonlinear speaker distortion. The existing NLMS has no delay
  search and costs a shift of its whole history per sample.
- **WebRTC AEC3** (what Chrome, Teams-class and Zoom-class clients are built on): delay estimator, partitioned-block
  frequency-domain adaptive filter, echo audibility and a nonlinear suppressor. A maintained pure-Rust port,
  `sonora` (BSD-3-Clause), builds for every target with no C++ toolchain, so ADR-004's concern about cross-compiling
  WebRTC for ARM64 Windows does not apply.

## Decision
- Run AEC3 (echo canceller only; no noise suppression, no gain control, no resampling) on the microphone stream with
  the loopback stream as the reference, through `sprk_audio::echo::EchoPath`, owned by `Session`. 10 ms frames at 16 kHz.
  Mic audio waits at most 300 ms for its reference, then is processed without one, so a missing reference never
  stalls the mic. Active only while both sources are on; restarted on pause, resume and source toggles.
- `LiveCapture` pads a stream that falls more than 400 ms behind the wall clock with silence, so both streams share a
  timeline and dedupe overlaps line up.
- Dedupe (layer 3) stays as the backstop for anything the canceller leaves.
- No new setting (CLAUDE.md rule 8). Headsets need no special case: with no echo there is nothing to cancel.

## Consequences
- Synthetic tests (`sprk_audio::echo`): 43 dB echo reduction with a 100 ms path and a distorted speaker model; 86%
  of the user's speech kept when talking over playback; a silent reference leaves the mic untouched.
- CPU is small (partitioned FFT filter, a few percent of one core). Must still be measured on the Snapdragon and Intel
  laptops (SPEC 13.3 gates).
- Real-room performance is unproven until the bleed test (SPEC 13.2) runs on laptops.
- A tab or app that plays through a different output than the one looped back is not heard by the reference and is
  still caught only by dedupe.

## Update after alpha.2 (same day)
Alpha.2 still sent a video played through the speakers to Me. Measuring AEC3 on synthetic audio showed why: it
removes echo only when the echo arrives between about 50 and 450 ms after the reference (0.7 dB reduction outside
that range, 43 to 67 dB inside). Our two streams are paired by sample count, and their real offset depends on
device buffering and on silence the loopback never delivered, so in real use the echo fell outside what AEC3 can lock to.

Added to `EchoPath`:
- The mic is held back 250 ms so the reference leads it, and a delay tracker (envelope cross-correlation, every 0.5 s,
  -0.8 to +1.2 s) re-centers the offset on 250 ms when it leaves 80 to 450 ms (delay the mic or the reference).
  Measured reduction is 65 dB for offsets from -400 to +1000 ms (it was 0.7 dB outside the AEC3 range).
- A backstop gate on the canceller's output: if what is left still follows the loudness pattern of the speakers
  (correlation of 0.75 or more over 2.5 s at a 80 to 450 ms delay) it is muted, with a 300 ms hangover. A talker with their own
  rhythm does not trigger it (85% of speech kept when talking over playback in the test).
- The 250 ms hold makes Me lines stamp up to about 0.25 s early. Dedupe already allows 2 s.
Costs a few envelope correlations per second (about 100k multiply-adds each), no allocation on the audio thread
(the session thread, not the capture callback).

## Update after alpha.4 (round 3)
Alpha.4 still let a video playing through the speakers show up as Me fairly often. A new test with a reverberant,
clipping speaker model and pauses between phrases showed two gaps. First, the loudness-pattern gate needs about 3 s
of history and 2.5 s of agreement, so a short phrase was over before it engaged. Second, AEC3 takes seconds to
settle, and the first seconds of every playback leak through it (about 10 dB of reduction instead of 60).

Added: an expected-bleed suppressor in `EchoPath`. The mic hears the speakers at a roughly steady fraction of what
they play (the coupling). Every 50 ms the mic level over the last 300 ms is compared with the reference level that
played one echo delay earlier; the coupling is the 35th percentile of those ratios over the last 15 s of playback
(80th percentile until 5 s have been seen, because the lowest ratios early on are the rising edge of a phrase). Mic
sound that is not at least 2.5 times (8 dB) above `coupling x reference` is muted with an 80 ms hangover. The user's
voice at a laptop mic is normally well above that; speech over playback is kept (84% in the test), speech that is
quieter than the bleed is not.

Decisions on the questions raised with this round:
- **Noise gate threshold:** not added as a setting. A fixed level cannot tell bleed from quiet speech (a loud video
  is louder than a quiet user), and the suppressor already adapts the level to the session. Fewer controls (SPEC 8).
- **Input vs output similarity:** already used three ways (AEC3 waveform, loudness-pattern gate, text dedupe) and
  now a fourth, the level relation. Spectral coherence was considered and left out: more CPU for little gain over
  what the tests show.
- **Input device picker:** added to Settings (SPEC 8.5 already lists mic device). Useful on its own and as a test
  tool: a headset mic sits close to the user, so the user's voice is far above the bleed.
- Known limit: the first second of the first playback in a session is not suppressed (nothing is learned yet).

## Update after alpha.6 (round 4): the streams were not lined up
Rounds 1 to 3 were tested by feeding the canceller the microphone and the loopback in perfect step. A new end-to-end
test (`crates/sprk-audio/tests/bleed_sim.rs`) feeds it through `LiveCapture` the way the app does: loopback that sends
nothing while silent, 10 ms device packets, and a session thread that is sometimes busy for 0.3 to 1.5 s (decoding).
It reproduced the field report: 3% to 100% of each phrase still leaked (most phrases over 30%), while the same audio
in step leaked nothing after the first phrase. Three causes:

1. **Audio placed by read time, not arrival time.** After a silence the loopback's audio was placed relative to when
   the session thread read it, with the stall padding leaving it 200 to 400 ms behind the clock. Each restart of
   playback moved the echo by a random few hundred ms, often outside the 50 to 450 ms AEC3 can follow, and the delay
   tracker needs seconds to re-center. When playback stopped while the thread was busy, the silence was padded in
   front of the last phrase instead of after it.
2. **Microphone before speakers.** Each poll queued the microphone's audio first. After a busy stretch the canceller
   had seconds of microphone audio and none of the matching speaker audio yet, gave up waiting after 300 ms, and
   processed the microphone with no reference.
3. **Reference slip.** Microphone audio processed without its reference consumed no reference, so the reference that
   arrived later was matched with later microphone audio: a permanent shift until the tracker caught it.

Decisions:
- `PcmStream::read_packets` returns device packets with their arrival time (the cpal backend stamps each callback,
  allocation-free, into a second lock-free ring). `LiveCapture` places a packet that follows a gap of 60 ms or more
  at its arrival time minus the device's usual delay (learned while audio flows), and fills silence after a phrase,
  never in front of it. Streams that cannot tell (Linux `parec`, which never goes quiet) keep the old padding.
- `LiveCapture` queues system audio before microphone audio in each poll.
- `EchoPath` keeps a reference debt: reference samples whose time was already processed without them are dropped on
  arrival, so the streams stay lined up sample for sample.
- The session restarts the canceller when a capture device changes, goes away or comes back.
- Dedupe (layer 3) also drops a 2 or 3 word Me line that is the same words in a row as an Others line, at about the
  moment those words were played (0.8 s). The same words said later, as a reply, are kept; one-word lines are not
  judged this way. This reverses the round-1 choice to leave all short lines alone: a short phrase at the exact
  moment it was played is the speakers.

Result in the simulation (mic delay 10 to 120 ms, loopback 10 to 40 ms): after the first phrase, at most 9% of any
phrase is cut by less than 20 dB (it was 3% to 100%), typically 30 to 60 dB of echo removed, and the user's speech in
the gaps and over playback kept (0.6 to over 0.9 of its level). The first phrase of a session is still the weak spot
(nothing learned yet; dedupe covers it). Real devices may deliver differently from the simulation, so the speakers-on
laptop check (SPEC 13.2) is still the test that counts.


# ADR-014: Stream separation, device routing and cancellation options

Status: accepted (alpha.5). Related: ADR-013 (AEC3), SPEC 4.1 and 4.2.

## Context
A Bluetooth headset in a real meeting produced Me and Others lines "seemingly at random", and Me lines appeared even with the
headset mic muted. John asked whether the two streams could be processed completely separately, tagged at capture, and
what free or standard software could cancel one against the other.

## Findings
**The streams are already separate and tagged.** Each source (`Source::Mic`, `Source::System`) has its own capture stream,
its own VAD and its own utterance queue in `Session`. The label comes from the stream an utterance came from (mic = Me,
loopback = Others); no text, mix or model decides it. So a Me line can only exist because the *microphone endpoint we
opened* delivered that sound. Muting the mic in a call app or on the headset does not stop that if we are reading a
different microphone (the laptop's own array hears the user, the room and the other side).

**Why separate recognizers are not needed.** The model is loaded once and decodes one finished utterance at a time. A second
instance would double model memory (hundreds of MB for the default model) and take about as much CPU as the first
whenever both sides talk, and would change nothing about attribution, which is already per stream. Running them in
parallel is viable on a big machine but not worth it on the target laptops.

**Likely cause of the report.** We opened the *console* default devices (cpal's default). A Bluetooth headset appears as two
endpoints (stereo for music, hands-free for calls) and a call app plays and captures on the *communications* defaults.
Windows does not always make the headset's hands-free mic the console default input, so we kept transcribing the laptop mic
and looped back an endpoint the call never used. SPEC 4.1 already said "default communications device" for the mic; the
code did not do that.

## Decisions
1. **Windows defaults by role.** The mic and the looped-back output default to the *communications* endpoints when one
   exists (`sprk_audio::endpoints`), else the console default. Read-only.
2. **Device pickers** for both streams in Settings (microphone, and the output that is looped back), so a wrong default can be
   corrected. An unplugged pick falls back to the default and returns when plugged back in.
3. **The devices in use are named in the transcript** ("Microphone: ...", "Others audio: ...") at the start and when they
   change. Device names go only into the transcript file (CLAUDE.md rule 4), never logs.
4. Headset use is protected by tests: with no acoustic path, the user's speech while the other side talks is kept.
5. **Echo cancellation stays WebRTC AEC3** (ADR-013).

## Options considered for cancelling one stream against the other
| Option | License | Verdict |
|---|---|---|
| WebRTC AEC3 (pure Rust port, `sonora`) | BSD-3 | **Used.** Same family as Chrome, Teams and Zoom; low CPU; needs the loopback as reference |
| SpeexDSP echo canceller | BSD | Older and weaker on nonlinear laptop speakers; no gain over AEC3 |
| Windows communications AEC (`IAcousticEchoCancellationControl`, Voice Capture DSP) | OS | Needs a communications capture stream and driver support, and the reference render endpoint to be chosen; availability varies by laptop. Kept as a possible later layer (SPEC 4.2 layer 1), not relied on |
| DTLN-aec (neural, small LSTM) | MIT | Better on heavy distortion, but needs a neural runtime on the capture path and training data we cannot tune here. Revisit if AEC3 plus suppression proves insufficient on the laptops |
| RNNoise and other denoisers | BSD | Remove noise, not a second speaker: they do not separate speakers or cancel playback |
| ANC-style "negate the output from the input" | n/a | That is what AEC does, using the loopback as the reference; it needs an acoustic path, so it does nothing for a headset (nothing leaks) |
| Speaker embeddings / separation models | various | Heavier, more models to ship, and wrong for a headset where the mic simply must not be the wrong device |

With a headset there is no acoustic echo to cancel; the fix for that case is reading the right endpoints, not more
signal processing.

## Consequences
- Needs a real-headset check on the laptop (SPEC 13.2 manual checks): connect the headset, start a session, confirm the
  two device lines name the headset, speak, mute the headset mic and confirm no Me lines.
- If the call app uses endpoints that differ from both defaults, the pickers cover it.

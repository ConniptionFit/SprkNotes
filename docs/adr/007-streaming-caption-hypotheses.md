# ADR-007: Streaming caption hypotheses

Status: accepted (2026-11-20, John's request). Builds on ADR-006. Settling and locking superseded in part by ADR-008.

## Context
ADR-006 sent whole replacement lines every ~0.7 s with no hint of which words were reliable, and the finished line only appeared after the commit delay. John wants captions that show words immediately, correct earlier words as confidence grows, and never affect the accuracy of the saved transcript.

## Decision
- Each caption decode is a hypothesis for the whole in-progress line. Words that two consecutive hypotheses agree on (ignoring case and punctuation, leading run only) are `stable`; the rest are tentative and may change. The caption carries `text` and `stable` (a leading word count), so the UI can dim tentative words and rewrite them freely.
- Decodes run every 0.45 s at the fastest (was 0.7 s), only when at least 0.2 s of new audio arrived. The cost backoff and auto-off from ADR-006 are unchanged.
- When the VAD closes an utterance, the committed decode (the same text that goes to the transcript) is sent at once as a `final` caption with every word stable. The overlay shows finished lines from these, so there is no wait for the commit delay and no duplicate.
- The saved transcript is untouched: it still comes only from finished utterances through the commit path. Captions add no decode of their own for finals and are never written or logged (rules 1, 4, 6, 13).
- The UI reveals new words one after another (about 70 ms apart; none with reduced motion).

## Consequences
Finals cost nothing extra. Words at the end of a hypothesis stay tentative until a later decode agrees or the line finishes, which is the intended "corrects itself" look. A true streaming (transducer) engine could later replace the re-decode behind the same `Caption` shape.

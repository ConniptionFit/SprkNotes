# ADR-008: Caption lock-in, display delay and bottom-anchored transcript

Status: accepted (2026-11-20, John chose "Typewriter + sentence lock"). Builds on ADR-006 and ADR-007; replaces how ADR-007 decides which words are settled. Research and mockups: `redesign/livelock/` in the project files.

## Context
Non-stop speech (a lecture, a video) never gives the voice detector a pause, so the live line grew for minutes. The 10 s decode window also slides, so the "leading words both decodes agree on" rule of ADR-007 broke as the window's left edge moved, and the whole line showed as unsure. John wants words typed out quickly, confident text in the normal color that stays, sentences that lock in, a short delay (under 1 s) for steadier text, and a transcript that fills from the bottom.

## Decision
- **Align, do not slide.** Each hypothesis is lined up with the last locked words (up to 8, at least 2 matching), so only words after them are new. The window moving forward no longer unsettles anything.
- **Settle, then lock.** A word two hypotheses in a row agree on is settled. Words lock (never change in the live view again) at a confirmed sentence end followed by another word, when a word has been on screen 1.5 s, or after 0.5 s of quiet. A live row also seals at 40 locked words.
- **Sealed rows.** Locked sentences leave the live row as `sealed` rows in the caption event, with an utterance number `utt`. The UI shows them as normal rows under the committed lines. The final caption for the same utterance (the committed decode) replaces its rows silently, sentence by sentence, and a committed line removes rows it covers (60% word overlap, same side Me or Others). Rows left over expire after 45 s. The saved transcript is untouched: it still comes only from finished utterances (rules 1, 4, 6, 13); nothing here is stored or logged.
- **Display delay (UI only).** A tentative word shows only after standing unchanged for 0.2 s (Fast), 0.4 s (Steady, default) or 0.6 s (Smoothest), set in Settings. Words that flip inside the gate never appear. With the 0.45 s decode interval the lag is about 0.6 to 0.9 s on Steady. Visible text is typed at about 14 ms per character, faster when behind; reduced motion shows whole words. Only the last word or two are soft (60% opacity), with no underline.
- **Bottom-anchored box.** The transcript fills from the bottom; new rows push older ones up. The live row is always last.
- **Follow fix.** Follow turns off only when the reader scrolls up (wheel up, touch, PageUp/Home/Up, scrollbar drag). App scrolling and content growing or shrinking never turn it off. A resize observer keeps the end in view while following, with instant (not animated) scrolling. Reaching the bottom, the pill or End turns follow back on.

## Consequences
No engine or model change: word timestamps are not needed because age is measured on screen. A repeated phrase directly at a lock point could, rarely, align one word early; the final caption corrects the rows. The overlay drawer shows sealed sentences as lines too, replaced by the final caption.

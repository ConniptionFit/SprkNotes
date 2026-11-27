// Speaker turns in the live transcript. A turn is one speaker's run of lines with nothing in
// between; the speaker's name shows once, on its first line. A mark, note or event ends a turn.

/**
 * For each entry, whether it starts a new turn. `speakers` holds the speaker of each entry, or
 * null for a mark, note or event. `before` is the speaker of whatever sits just above the first
 * entry (null when nothing does or it is not speech).
 */
export function turnStarts(speakers: readonly (string | null)[], before: string | null = null): boolean[] {
  return speakers.map((s, i) => s !== null && s !== (i === 0 ? before : speakers[i - 1]));
}

/** The speaker of the last entry, for the list that follows it. */
export function lastSpeaker(speakers: readonly (string | null)[], before: string | null = null): string | null {
  return speakers.length > 0 ? speakers[speakers.length - 1] : before;
}

// "Keep transcripts for" (SPEC 6.6): the choices Settings offers and how a number of days reads.

/** Days, 0 being forever. An administrator may set another number; it still shows (see retentionText). */
export const RETENTION_CHOICES: readonly number[] = [0, 30, 90, 365];

/** "Forever", "30 days", "1 year", "2 years", "1 day". */
export function retentionText(days: number): string {
  if (days <= 0) return "Forever";
  if (days % 365 === 0) return days === 365 ? "1 year" : `${days / 365} years`;
  return days === 1 ? "1 day" : `${days} days`;
}

/** Live pane scroll rules (SPEC 8.2). Pure so they can be tested without a browser. */
export const FOLLOW_THRESHOLD_PX = 48;
/** A scroll event this soon after the reader's own input counts as the reader's. */
export const USER_INPUT_WINDOW_MS = 700;

export function isNearBottom(
  scrollTop: number,
  clientHeight: number,
  scrollHeight: number,
  threshold = FOLLOW_THRESHOLD_PX,
): boolean {
  return scrollHeight - (scrollTop + clientHeight) <= threshold;
}

export interface FollowState {
  follow: boolean;
  /** Lines that arrived while follow mode was off. */
  unseen: number;
}

export const initialFollow: FollowState = { follow: true, unseen: 0 };

/**
 * A scroll event. Near the bottom always turns follow on. Away from the bottom turns it off only
 * when the reader caused it: content growing under the pane, or the app's own scrolling, never
 * does (that was the "stops following after a while" bug).
 */
export function onScroll(s: FollowState, nearBottom: boolean, byReader: boolean): FollowState {
  if (nearBottom) return initialFollow;
  return byReader ? { ...s, follow: false } : s;
}

/** New lines arrived. While following nothing is counted. */
export function onLines(s: FollowState, added: number): FollowState {
  return s.follow ? s : { ...s, unseen: s.unseen + added };
}

/** "Return to live" pill or the End key. */
export function returnToLive(): FollowState {
  return initialFollow;
}

/** Keys that move the view up, away from the live end. */
export const UP_KEYS = ["ArrowUp", "PageUp", "Home"];

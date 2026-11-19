/** Live pane scroll rules (SPEC 8.2). Pure so they can be tested without a browser. */
export const FOLLOW_THRESHOLD_PX = 48;

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

/** User scrolled: near the bottom turns follow on (and clears the count), away turns it off. */
export function onScroll(s: FollowState, nearBottom: boolean): FollowState {
  return nearBottom ? initialFollow : { ...s, follow: false };
}

/** New lines arrived. While following nothing is counted. */
export function onLines(s: FollowState, added: number): FollowState {
  return s.follow ? s : { ...s, unseen: s.unseen + added };
}

/** "Return to live" pill or the End key. */
export function returnToLive(): FollowState {
  return initialFollow;
}

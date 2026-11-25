// Which shell the shared UI is running in (ADR-015 Decision 6). The phone app is an Android
// WebView, whose user agent always names Android; the desktop app's never does. Synchronous, so
// the first paint already has the right layout and theme.
export function isPhone(ua: string = typeof navigator === "undefined" ? "" : navigator.userAgent): boolean {
  return /\bAndroid\b/i.test(ua);
}

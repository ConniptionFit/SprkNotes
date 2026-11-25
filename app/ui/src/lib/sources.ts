// Device menu for a sound source (Me / Others). Pure, so it is testable without a window.

export type DeviceOption = { value: string | null; label: string; missing: boolean };

/** "System default" first, then the devices found now. A saved pick that is not plugged in
 * stays in the list (marked missing) so the choice is never lost silently. */
export function menuOptions(found: string[], picked: string | null): DeviceOption[] {
  const seen = new Set<string>();
  const out: DeviceOption[] = [{ value: null, label: "System default", missing: false }];
  for (const name of found) {
    if (!name || seen.has(name)) continue;
    seen.add(name);
    out.push({ value: name, label: name, missing: false });
  }
  if (picked && !seen.has(picked)) out.push({ value: picked, label: `${picked} (not connected)`, missing: true });
  return out;
}

/** Short label for the button's tooltip. */
export function deviceLabel(picked: string | null): string {
  return picked ?? "System default";
}

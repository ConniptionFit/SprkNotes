import { describe, expect, it } from "vitest";
import { RETENTION_CHOICES, retentionText } from "./retention";

describe("retention choices", () => {
  it("offers forever first and reads each choice plainly", () => {
    expect(RETENTION_CHOICES.map(retentionText)).toEqual(["Forever", "30 days", "90 days", "1 year"]);
  });
  it("reads an administrator's own number too", () => {
    expect(retentionText(180)).toBe("180 days");
    expect(retentionText(730)).toBe("2 years");
    expect(retentionText(1)).toBe("1 day");
  });
});

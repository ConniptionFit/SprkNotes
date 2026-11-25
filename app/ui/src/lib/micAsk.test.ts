import { describe, expect, it } from "vitest";
import { appList, asAsk, askText, skipLabel, withSkipped } from "./micAsk";

const zoom = { id: "zoom.exe", name: "Zoom" };
const teams = { id: "microsoft.teams_8wekyb3d8bbwe", name: "Microsoft Teams" };
const discord = { id: "discord.exe", name: "Discord" };

describe("mic prompt wording", () => {
  it("names the app holding the microphone", () => {
    expect(askText([zoom])).toBe("Zoom is using your microphone. Start a transcript?");
    expect(askText([zoom, teams])).toBe("Zoom and Microsoft Teams are using your microphone. Start a transcript?");
    expect(askText([])).toBe("Your microphone is in use. Start a transcript?");
  });

  it("lists three apps with commas", () => {
    expect(appList([zoom, teams, discord])).toBe("Zoom, Microsoft Teams and Discord");
  });

  it("labels the skip button by app", () => {
    expect(skipLabel([zoom])).toBe("Don't ask for Zoom");
    expect(skipLabel([zoom, teams])).toBe("Don't ask for these apps");
  });
});

describe("skip list", () => {
  it("adds each app once", () => {
    const list = withSkipped([{ id: "ZOOM.EXE", name: "Zoom" }], [zoom, teams]);
    expect(list.map((a) => a.id)).toEqual(["ZOOM.EXE", teams.id]);
  });
});

describe("shell payload", () => {
  it("reads both the old boolean and the new question", () => {
    expect(asAsk(true)).toEqual({ up: true, apps: [] });
    expect(asAsk({ up: true, apps: [zoom, { id: 3 }] })).toEqual({ up: true, apps: [zoom] });
    expect(asAsk(null)).toEqual({ up: false, apps: [] });
  });
});

import { describe, expect, it } from "vitest";
import { champions } from "./champions";
import { percent, points, reasonText, reasonTone, seconds } from "./format";

describe("format", () => {
  it("formats numbers for people", () => {
    expect(percent(0.5234)).toBe("52.3%");
    expect(points(0.0213)).toBe("+2.1");
    expect(points(-0.004)).toBe("-0.4");
    expect(seconds(1001)).toBe("2");
    expect(seconds(-50)).toBe("0");
  });

  it("explains reasons with champion names and a tone", () => {
    const ahri = champions.name(103);
    expect(ahri).toBe("Ahri");
    expect(reasonText({ kind: "matchup", enemy: 103, delta: -0.02 }, champions)).toBe("weak into Ahri (-2.0)");
    expect(reasonText({ kind: "threat", ally: 103, delta: 0.031 }, champions)).toBe("beats your Ahri (+3.1)");
    expect(reasonTone({ kind: "matchup", enemy: 1, delta: -0.01 })).toBe("bad");
    expect(reasonTone({ kind: "winRate", position: "top", winRate: 0.5 })).toBe("neutral");
  });
});

describe("champions", () => {
  it("searches ignoring case, accents and punctuation", () => {
    expect(champions.search("kaisa").map((c) => c.name)).toEqual(["Kai'Sa"]);
    expect(champions.search("LEE s").map((c) => c.name)).toEqual(["Lee Sin"]);
    expect(champions.search("").length).toBe(champions.list.length);
  });

  it("names unknown champions by ID and builds Data Dragon icon URLs", () => {
    expect(champions.name(999_999)).toBe("#999999");
    expect(champions.icon(62)).toMatch(/\/img\/champion\/MonkeyKing\.png$/);
  });
});

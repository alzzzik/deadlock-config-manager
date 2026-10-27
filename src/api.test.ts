import { describe, expect, it } from "vitest";
import { dayAgo, daysSince, fovFor, formatBackupId, isRecent, isSamePack, matchesSearch, pinKey, sortStore, updateNotes, PackInfo, AppliedRecord, StoreEntry } from "./api";

const pack = (over: Partial<PackInfo> = {}): PackInfo => ({
  repo: "alzzzik/alz-deadlock-cfg",
  id: "stable",
  name: "Stable",
  description: "Low input lag",
  folder: "stable",
  lastCommitDate: null,
  revision: "abc",
  commitMessage: "Tune shadows",
  htmlUrl: "https://github.com/alzzzik/alz-deadlock-cfg/tree/main/stable",
  ...over,
});

const record = (over: Partial<AppliedRecord> = {}): AppliedRecord => ({
  repo: "alzzzik/alz-deadlock-cfg",
  packId: "stable",
  packName: "Stable",
  revision: "abc",
  appliedAt: "2026-10-20T10:00:00Z",
  ...over,
});

describe("matchesSearch", () => {
  it("matches name, description, repo and commit message case-insensitively", () => {
    expect(matchesSearch(pack(), "STABLE")).toBe(true);
    expect(matchesSearch(pack(), "input lag")).toBe(true);
    expect(matchesSearch(pack(), "alzzzik")).toBe(true);
    expect(matchesSearch(pack(), "shadows")).toBe(true);
  });

  it("treats an empty query as a match and rejects unrelated text", () => {
    expect(matchesSearch(pack(), "  ")).toBe(true);
    expect(matchesSearch(pack(), "optilock")).toBe(false);
  });
});

describe("isSamePack", () => {
  it("falls back to the first repo for records saved before repos were stored", () => {
    expect(isSamePack(record({ repo: null }), pack(), "alzzzik/alz-deadlock-cfg")).toBe(true);
    expect(isSamePack(record({ repo: null }), pack(), "dacooderr/OptiLock")).toBe(false);
  });

  it("needs both the id and the repo to match", () => {
    expect(isSamePack(record(), pack({ id: "max" }), undefined)).toBe(false);
    expect(isSamePack(null, pack(), undefined)).toBe(false);
  });
});

describe("pinKey and formatBackupId", () => {
  it("builds a repo#id pin key", () => {
    expect(pinKey(pack())).toBe("alzzzik/alz-deadlock-cfg#stable");
  });

  it("leaves unknown backup ids untouched", () => {
    expect(formatBackupId("not-a-date")).toBe("not-a-date");
    expect(formatBackupId("2026-10-20_02-10-05")).not.toBe("2026-10-20_02-10-05");
  });
});

describe("fovFor", () => {
  it("matches the community table and interpolates between it", () => {
    expect(fovFor(2.15)).toBe(90);
    expect(fovFor(2.5)).toBe(100);
    expect(fovFor(3.5)).toBe(120);
    expect(fovFor(2.4)).toBe(97);
    expect(fovFor(1.33)).toBe(70);
    expect(fovFor(0)).toBeNull();
  });
});

const NOW = Date.parse("2026-10-23T15:00:00Z");
const gb = (id: number, updated: string | null, recommended = false): StoreEntry => ({ kind: "gamebanana", id, title: `gb ${id}`, description: "", author: "", updated, recommended });

describe("store dates", () => {
  it("counts whole days from a YYYY-MM-DD date", () => {
    expect(daysSince("2026-10-23", NOW)).toBe(0);
    expect(daysSince("2026-10-22", NOW)).toBe(1);
    expect(daysSince("nonsense", NOW)).toBeNull();
    expect(daysSince(null, NOW)).toBeNull();
    expect(dayAgo("2026-10-23", NOW)).toBe("today");
    expect(dayAgo("2026-10-22", NOW)).toBe("yesterday");
    expect(dayAgo("2026-07-15", NOW)).toContain("3");
  });

  it("sorts newest first with undated entries last, and filters recent ones", () => {
    const sorted = sortStore([gb(1, "2026-02-05"), gb(2, null), gb(3, "2026-10-22"), gb(4, "2026-09-30")]);
    expect(sorted.map((e) => (e.kind === "gamebanana" ? e.id : 0))).toEqual([3, 4, 1, 2]);
    // Recommended entries come first, in list order, whatever their date.
    const withPicks = sortStore([gb(1, "2026-10-22"), gb(2, "2026-01-01", true), gb(3, "2026-10-20", true)]);
    expect(withPicks.map((e) => (e.kind === "gamebanana" ? e.id : 0))).toEqual([2, 3, 1]);
    expect(isRecent(gb(3, "2026-10-22"), NOW)).toBe(true);
    expect(isRecent(gb(5, "2026-07-24"), NOW)).toBe(true);
    expect(isRecent(gb(1, "2026-02-05"), NOW)).toBe(false);
    expect(isRecent(gb(2, null), NOW)).toBe(false);
  });
});

describe("updateNotes", () => {
  it("lists the release's change lines and skips the download text", () => {
    const body = "- Performance opens right away\n- Records matches again\n\nDownload **setup.exe** and run it.\n";
    expect(updateNotes(body)).toEqual(["Performance opens right away", "Records matches again"]);
    expect(updateNotes(undefined)).toEqual([]);
    expect(updateNotes("- a\n- b\n- c", 2)).toEqual(["a", "b"]);
  });
});

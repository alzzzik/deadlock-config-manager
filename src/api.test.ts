import { describe, expect, it } from "vitest";
import { fovFor, formatBackupId, isSamePack, matchesSearch, pinKey, PackInfo, AppliedRecord } from "./api";

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

import { describe, expect, it } from "vitest";
import { GUIDE_CHOICES, suggestChoice } from "./FirstRunGuide";

describe("suggestChoice", () => {
  it("suggests apex-complock only with CompLock's paks on an NVIDIA GPU", () => {
    expect(suggestChoice({ complock: true, mods: true, nvidia: true }).id).toBe("complock");
    expect(suggestChoice({ complock: true, mods: true, nvidia: false }).id).toBe("mods");
  });

  it("suggests the mod-friendly config when mods are installed, else the general one", () => {
    expect(suggestChoice({ complock: false, mods: true, nvidia: true }).id).toBe("mods");
    expect(suggestChoice({ complock: false, mods: false, nvidia: true })).toEqual({ id: "most", reason: null });
    expect(suggestChoice(null).id).toBe("most");
  });

  it("only points at alz folders that still exist", () => {
    const alz = GUIDE_CHOICES.filter((c) => c.repo === "alzzzik/alz-deadlock-cfg").map((c) => c.folder);
    expect(alz.sort()).toEqual(["apex-complock", "stable"]);
  });
});

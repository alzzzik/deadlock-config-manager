import { createContext, useContext } from "react";

/** Ranked-safety per pack (`owner/name#id` → engine entries changed), filled in after packs load. */
export type PackChecks = Record<string, number | null>;

export const PackChecksContext = createContext<PackChecks>({});

export const usePackCheck = (key: string): number | null | undefined => useContext(PackChecksContext)[key];

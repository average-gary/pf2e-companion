// IPC bindings to the Rust backend. Mirrors src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export interface SearchHit {
  id: string;
  title: string;
  type: string;
  snippet: string;
  score: number;
}

export interface EntitySummary {
  id: string;
  title: string;
  type: string;
  lens: string | null;
}

export interface AliasHit {
  legacy_name: string;
  remaster_name: string;
  category: string;
  notes: string | null;
}

export interface MiracleHit {
  miracle: string;
  reference: string;
  book: string;
  spell_name: string;
  tradition: string | null;
  sanctification: string | null;
  notes: string | null;
}

export type Difficulty =
  | "trivial"
  | "low"
  | "moderate"
  | "severe"
  | "extreme";

export interface XpBudgetResult {
  party_size: number;
  difficulty: Difficulty;
  xp_budget: number;
  per_pc_adjust: number;
  base_for_party_of_4: number;
}

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

export interface LensManifest {
  id: string;
  label: string;
  description: string;
}

export interface ImportReport {
  files_seen: number;
  files_imported: number;
  files_skipped: number;
  errors: string[];
}

export const search = (query: string, lens?: string) =>
  invoke<SearchHit[]>("search", { query, lens });

export const listEntities = (typeFilter?: string, lens?: string) =>
  invoke<EntitySummary[]>("list_entities", {
    typeFilter: typeFilter ?? null,
    lens: lens ?? null,
  });

export const schemaVersion = () => invoke<number>("schema_version");

export const lookupAlias = (name: string) =>
  invoke<AliasHit[]>("lookup_alias", { name });

export const lookupMiracle = (query: string) =>
  invoke<MiracleHit[]>("lookup_miracle", { query });

export const xpBudget = (partySize: number, difficulty: Difficulty) =>
  invoke<XpBudgetResult>("xp_budget", { partySize, difficulty });

export const creatureXp = (partyLevelDelta: number) =>
  invoke<number | null>("creature_xp", { partyLevelDelta });

export const validateStatblock = (statblock: unknown) =>
  invoke<ValidationResult>("validate_statblock", { statblock });

export const listLenses = () => invoke<LensManifest[]>("list_lenses");

export const importFoundryPack = (rootPath: string, license: string) =>
  invoke<ImportReport>("import_foundry_pack", { rootPath, license });

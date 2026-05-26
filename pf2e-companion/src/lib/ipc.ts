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

export interface EntityDetail {
  id: string;
  title: string;
  type: string;
  lens: string | null;
  license_provenance: string;
  source: string;
  frontmatter: Record<string, unknown>;
  body: string | null;
  statblock: Record<string, unknown> | null;
}

export const getEntity = (id: string) =>
  invoke<EntityDetail | null>("get_entity", { id });

// === Phase 3 ===========================================================

export interface Campaign {
  id: string;
  name: string;
  default_lens: string | null;
  entity_count: number;
}

export interface CrudResult {
  id: string;
  file_path: string;
}

export interface EntityInputDTO {
  campaign_id: string;
  type: string;
  title: string;
  lens?: string | null;
  license_provenance?: string | null;
  body?: string | null;
  frontmatter: Record<string, unknown>;
}

export interface EntityPatchDTO {
  title?: string | null;
  lens?: string | null;
  license_provenance?: string | null;
  body?: string | null;
  frontmatter?: Record<string, unknown> | null;
}

export interface RelationRow {
  from_id: string;
  edge_type: string;
  to_id: string;
}

export const listCampaigns = () => invoke<Campaign[]>("list_campaigns");

export const createCampaign = (name: string, defaultLens: string | null = null) =>
  invoke<Campaign>("create_campaign", { name, defaultLens });

export const createEntity = (input: EntityInputDTO) =>
  invoke<CrudResult>("create_entity", { input });

export const updateEntity = (id: string, patch: EntityPatchDTO) =>
  invoke<CrudResult>("update_entity", { id, patch });

export const deleteEntity = (id: string) =>
  invoke<void>("delete_entity", { id });

export const addRelation = (fromId: string, edgeType: string, toId: string) =>
  invoke<void>("add_relation", { fromId, edgeType, toId });

export const deleteRelation = (fromId: string, edgeType: string, toId: string) =>
  invoke<void>("delete_relation", { fromId, edgeType, toId });

export const listRelations = (entityId: string) =>
  invoke<RelationRow[]>("list_relations", { entityId });

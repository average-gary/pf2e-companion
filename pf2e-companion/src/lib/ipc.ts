// IPC bindings to the Rust backend. Mirrors src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

export interface SearchHit {
  id: string;
  title: string;
  type: string;
  snippet: string;
  score: number;
  /** "fts" | "vec" | "both" — which retriever surfaced this hit (Phase 6 § C). */
  source: string;
}

export interface RagIndexStats {
  indexed: boolean;
  entities: number;
  chunks: number;
  provider: string | null;
  model: string | null;
}

export interface RagEmbedReport {
  provider: string;
  model: string;
  entities_processed: number;
  chunks_embedded: number;
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

// === Phase 6 — LLM (off by default, BYO key) ==========================

export type LlmProviderKind = "anthropic" | "ollama";

export interface LlmConfig {
  provider: LlmProviderKind;
  model: string;
  base_url?: string | null;
}

export interface LlmStatus {
  configured: boolean;
  provider: LlmProviderKind | null;
  model: string | null;
  key_present: boolean;
}

export type ChatRole = "system" | "user" | "assistant" | "tool";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

/**
 * Event payload streamed from the agent loop. Five variants:
 *   - `token`        — assistant prose fragment
 *   - `tool_start`   — model issued a tool call (UI may show spinner)
 *   - `tool_result`  — tool result was appended to the conversation
 *   - `done`         — final usage + iteration count
 *   - `error`        — fatal error; no further events for this session
 */
export type LlmTokenEvent =
  | {
      session_id: string;
      kind: "token";
      token: string;
    }
  | {
      session_id: string;
      kind: "tool_start";
      id: string;
      name: string;
      input: unknown;
    }
  | {
      session_id: string;
      kind: "tool_result";
      id: string;
      name: string;
      result: unknown;
      error: boolean;
    }
  | {
      session_id: string;
      kind: "done";
      input_tokens: number | null;
      output_tokens: number | null;
      cache_read_input_tokens: number | null;
      cache_creation_input_tokens: number | null;
      iterations: number;
    }
  | {
      session_id: string;
      kind: "error";
      message: string;
    };

export const llmStatus = () => invoke<LlmStatus>("llm_status");

export const llmConfigure = (
  config: LlmConfig,
  apiKey: string | null = null,
) => invoke<LlmStatus>("llm_configure", { config, apiKey });

export const llmClearConfig = () => invoke<LlmStatus>("llm_clear_config");

export const llmChat = (
  messages: ChatMessage[],
  opts: {
    system?: string | null;
    temperature?: number | null;
    maxTokens?: number | null;
    cacheSystem?: boolean;
  } = {},
) =>
  invoke<string>("llm_chat", {
    request: {
      messages,
      system: opts.system ?? null,
      temperature: opts.temperature ?? null,
      max_tokens: opts.maxTokens ?? null,
      cache_system: opts.cacheSystem ?? false,
    },
  });

// === Phase 6 § C — RAG ====================================================

export const ragIndexStats = () => invoke<RagIndexStats>("rag_index_stats");

export const ragReindex = () => invoke<RagEmbedReport>("rag_reindex");

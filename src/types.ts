// 与 Rust 后端 DTO 一一对应（serde camelCase）

export type Mirror = "official" | "bmclapi";

export type TaskStatus =
  | "queued"
  | "running"
  | "paused"
  | "completed"
  | "failed"
  | "canceled";

export type Stage =
  | "manifest"
  | "version_json"
  | "libraries"
  | "assets"
  | "forge"
  | "verifying"
  | "finished";

export interface VersionSummary {
  id: string;
  type: string; // release | snapshot | old_beta | old_alpha
  url: string;
  releaseTime: string;
  sha1?: string;
}

export interface TaskSnapshot {
  taskId: string;
  versionId: string;
  gameDir: string;
  mirror: Mirror;
  status: TaskStatus;
  stage?: Stage;
  downloadedBytes: number;
  totalBytes: number;
  speedBps: number;
  currentFile?: string;
  filesDone: number;
  filesTotal: number;
  error?: string;
  createdAt: number;
  finishedAt?: number;
}

export type VersionSource =
  | { id: string }
  | { mcVersion: string; loaderVersion: string }
  | { mcVersion: string; forgeVersion: string; branch?: string | null };

export interface FabricLoaderSummary {
  loader: { version: string; stable: boolean };
}

export interface ForgeBuildSummary {
  mcVersion: string;
  forgeVersion: string;
  branch?: string | null;
  artifactId: string;
  versionId: string;
  build: number;
  modified: string;
}

export interface ReportEntry {
  path: string;
  url?: string;
  expectedSha1?: string;
  actualSha1?: string;
  size?: number;
}

export interface VerificationReport {
  versionId: string;
  gameDir: string;
  ok: boolean;
  checkedFiles: number;
  missing: ReportEntry[];
  corrupted: ReportEntry[];
  totalBytes: number;
}

export interface AppSettings {
  onboarded: boolean;
  /** @deprecated 旧单目录字段（仅兼容读取） */
  gameDir?: string | null;
  gameDirs: string[];
  activeGameDir?: string | null;
  mirror: Mirror;
  concurrency: number;
  javaPath?: string | null;
}

import { invoke } from "@tauri-apps/api/core";

// 后端 AppError 序列化后的形状：code 用于分支判断，message/hint 直接给用户看。
export interface AppErrorShape {

  code: string;
  message: string;
  hint: string | null;
}

export function isAppErrorShape(e: unknown): e is AppErrorShape {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    typeof (e as { message: unknown }).message === "string"
  );
}

export function toAppError(e: unknown): AppErrorShape {
  if (isAppErrorShape(e)) return e;
  return {
    code: "unknown",
    message: typeof e === "string" ? e : JSON.stringify(e),
    hint: null,
  };
}

export interface DbStatus {
  data_dir: string;
  db_file: string;
  journal_mode: string;
  schema_version: number;
  fts5_available: boolean;
}

export const dbStatus = () => invoke<DbStatus>("db_status");

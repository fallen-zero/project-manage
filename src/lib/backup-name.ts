// 默认包名：pfm-backup-<YYYYMMDD>-<HHMMSS>.zip。
// 刻意不用 ISO 的冒号形态（`2026-10-13T21:30:12`）—— `:` 在 Windows 文件名里非法，
// 而这个名字是用户「另存为」对话框里直接带着的默认值。用 UTC 与 Rust 侧 manifest 的
// createdAt 同口径（那边也是 strftime('now')）。
// 本文件刻意不 import 任何运行时值：这样它能被 `node --test` 直跑（先例 src/lib/search-order.ts）。
const p2 = (n: number) => String(n).padStart(2, "0");

export function backupFileName(now: Date): string {
  const y = now.getUTCFullYear();
  const rest = `${p2(now.getUTCMonth() + 1)}${p2(now.getUTCDate())}`;
  const hms = `${p2(now.getUTCHours())}${p2(now.getUTCMinutes())}${p2(now.getUTCSeconds())}`;
  return `pfm-backup-${y}${rest}-${hms}.zip`;
}

import test from "node:test";
import assert from "node:assert/strict";
import { backupFileName } from "../src/lib/backup-name.ts";

// Windows 文件名里 `:` 非法，而包名是给用户直接另存到桌面上的东西 ——
// 这一格与 M7 的 profile URL 必须编 %20 是同一类「路径形状要到本机量」的账。
test("backupFileName 不含 Windows 非法字符且带 .zip", () => {
  const name = backupFileName(new Date("2026-10-13T21:30:12Z"));
  assert.equal(name, "pfm-backup-20261013-213012.zip");
  for (const bad of [":", "*", "?", '"', "<", ">", "|"]) {
    assert.ok(!name.includes(bad), `包名里不能有 ${bad}：${name}`);
  }
});

// 月/日/时/分/秒都必须补零，否则 1 月的包名比 11 月的短、按名字排序会乱。
test("backupFileName 补零到固定长度，字典序即时间序", () => {
  const jan = backupFileName(new Date("2026-01-05T09:07:03Z"));
  const nov = backupFileName(new Date("2026-11-20T23:59:59Z"));
  assert.equal(jan, "pfm-backup-20260105-090703.zip");
  assert.ok(jan.length === nov.length, `两个长度要相同：${jan} vs ${nov}`);
  assert.ok(jan < nov, "字典序必须等于时间序");
});

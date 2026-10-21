import test from "node:test";
import assert from "node:assert/strict";
import { defaultFileName } from "../src/lib/ledger-export-name.ts";

test("把 Windows 非法字符替换成下划线", () => {
  assert.equal(
    defaultFileName('甲/乙:丙*丁?戊"己<庚>辛|壬\\癸', "xlsx", "2026-10-21"),
    "台账-甲_乙_丙_丁_戊_己_庚_辛_壬_癸-2026-10-21.xlsx",
  );
});

test("去掉首尾空白与末尾的点", () => {
  assert.equal(defaultFileName("  政务云.  ", "md", "2026-10-21"), "台账-政务云-2026-10-21.md");
});

test("超长项目名截到 60 个字符，扩展名仍在", () => {
  const name = "长".repeat(80);
  const got = defaultFileName(name, "xlsx", "2026-10-21");
  assert.equal(got, `台账-${"长".repeat(60)}-2026-10-21.xlsx`);
});

test("空名回落到「项目」", () => {
  assert.equal(defaultFileName("   ", "md", "2026-10-21"), "台账-项目-2026-10-21.md");
});

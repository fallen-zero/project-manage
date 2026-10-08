// 首屏结果页的纯逻辑：段的存在性、三个截断计数的文案、空态判据、seq 守卫。
// 这一格刻意不 import 任何运行时值（`import type` 会被擦除），这样它能被 `node --test` 直跑，
// 不必给仓库添 jsdom / @testing-library。
import type { SearchBundle } from "@/types/search";

export type SectionKind = "projects" | "ledger" | "docs";

export const SECTION_TITLES: Record<SectionKind, string> = {
  projects: "项目档案",
  ledger: "信息台账",
  docs: "文档正文",
};

export interface SectionView {
  kind: SectionKind;
  title: string;
  /** 该段的截断/放宽说明；没有要说的就 null。 */
  note: string | null;
}

/** 连打两个字会发两次请求，后回来的旧的不能盖新的：带一个自增序号比较就够了。 */
export function isNewest(mine: number, latest: number): boolean {
  return mine === latest;
}

/** 簇内那一行的「还有 N 条」；后端已经截过，这里只负责说实话。 */
export function clusterNote(hidden: number): string | null {
  return hidden > 0 ? `还有 ${hidden} 条未显示` : null;
}

/** 三段的存在性与说明。顺序固定为 项目 → 台账 → 正文；**空段不产出**，但两段的「空」判据不同形：
 *  平铺段看 `projects.length`（有没有行），聚簇段看 `clusters.length`（有没有簇）—— 簇里 `items` 为空
 *  也照样产出，因为那一行的截断说明与放宽说明仍然要说。
 *  三个截断计数各有自己的说法：平铺段用「还有」，聚簇段用「另有」，段级说明里 relaxed 只追加一次。 */
export function bundleToSections(bundle: SearchBundle): SectionView[] {
  const out: SectionView[] = [];
  if (bundle.projects.length > 0) {
    out.push({
      kind: "projects",
      title: SECTION_TITLES.projects,
      note: bundle.projectsHidden > 0 ? `还有 ${bundle.projectsHidden} 个项目未显示` : null,
    });
  }
  if (bundle.ledger.clusters.length > 0) {
    out.push({
      kind: "ledger",
      title: SECTION_TITLES.ledger,
      note:
        bundle.ledger.hiddenClusters > 0 ? `另有 ${bundle.ledger.hiddenClusters} 个项目未显示` : null,
    });
  }
  if (bundle.docs.clusters.length > 0) {
    const bits: string[] = [];
    if (bundle.docs.hiddenClusters > 0) bits.push(`另有 ${bundle.docs.hiddenClusters} 个项目未显示`);
    if (bundle.relaxed) bits.push("含前缀放宽匹配");
    out.push({ kind: "docs", title: SECTION_TITLES.docs, note: bits.length ? bits.join("；") : null });
  }
  return out;
}

export type EmptyState = "results" | "not-indexed" | "no-hit";

/** 三段全空时要说哪一句。`indexedProjects === 0` 与「有索引但没命中」不能同一句话回答
 *  （M3 的 m8 教训：同一句文案盖两种失效，用户就分不清是还没建索引还是搜错了）。 */
export function emptyStateKind(bundle: SearchBundle): EmptyState {
  if (bundleToSections(bundle).length > 0) return "results";
  return bundle.indexedProjects === 0 ? "not-indexed" : "no-hit";
}

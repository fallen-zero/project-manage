import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { Badge } from "@/components/ui/badge";
import { DocPreviewDialog } from "@/components/doc-preview-dialog";
import { bundleToSections, clusterNote } from "@/lib/search-order";
import { SOURCE_LABELS, type FieldHit, type SearchBundle } from "@/types/search";
import type { DocHit } from "@/types/index";

function FieldRow({ hit }: { hit: FieldHit }) {
  return (
    <div className="flex items-center gap-2 rounded-md border bg-background px-3 py-2 text-sm">
      <div className="min-w-0 flex-1">
        <p className="truncate">{hit.title}</p>
        {hit.detail && (
          <p className="truncate font-mono text-xs text-muted-foreground" title={hit.detail}>
            {hit.detail}
          </p>
        )}
      </div>
      <Badge variant="outline" className="shrink-0">{SOURCE_LABELS[hit.source]}</Badge>
      <Badge variant="outline" className="shrink-0">{hit.projectName}</Badge>
      <Link
        to="/projects/$projectId"
        params={{ projectId: hit.projectId }}
        className="shrink-0 text-xs underline text-muted-foreground hover:text-foreground"
      >
        打开项目
      </Link>
    </div>
  );
}

/** 正文段的一行：点开才去磁盘重抽原文，所以这里只有路径与摘要。 */
function DocRow({ hit, onOpen }: { hit: DocHit; onOpen: (docId: string) => void }) {
  return (
    <button
      className="rounded-md border bg-background px-3 py-2 text-left text-sm hover:bg-muted/50"
      onClick={() => onOpen(hit.docId)}
    >
      <p className="truncate font-mono text-xs">{hit.path}</p>
      <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{hit.snippet}</p>
      {hit.matchedBy === "prefix" && <Badge variant="outline" className="mt-1">放宽匹配</Badge>}
    </button>
  );
}

/** 簇头那一行：项目名 + 簇内截断说明。两段的簇头同形，所以共用这一个小组件。 */
function ClusterHead({ name, hidden }: { name: string; hidden: number }) {
  const note = clusterNote(hidden);
  return (
    <p className="px-1 text-xs text-muted-foreground">
      {name}
      {note && <span className="ml-2">{note}</span>}
    </p>
  );
}

export function SearchBundleView({ bundle }: { bundle: SearchBundle }) {
  const [open, setOpen] = useState<{ docId: string; projectId: string } | null>(null);
  const sections = bundleToSections(bundle);

  return (
    <div className="grid gap-4">
      {sections.map((s) => (
        <section key={s.kind} className="grid gap-1.5">
          <h2 className="text-xs font-medium text-muted-foreground">
            {s.title}
            {s.note && <span className="ml-2 font-normal">{s.note}</span>}
          </h2>
          {s.kind === "projects" && bundle.projects.map((h) => <FieldRow key={h.id} hit={h} />)}
          {s.kind === "ledger" &&
            bundle.ledger.clusters.map((c) => (
              <div key={c.projectId} className="grid gap-1">
                <ClusterHead name={c.projectName} hidden={c.hidden} />
                {c.items.map((h) => (
                  <FieldRow key={h.id} hit={h} />
                ))}
              </div>
            ))}
          {s.kind === "docs" &&
            bundle.docs.clusters.map((c) => (
              <div key={c.projectId} className="grid gap-1">
                <ClusterHead name={c.projectName} hidden={c.hidden} />
                {c.items.map((h) => (
                  <DocRow
                    key={h.docId}
                    hit={h}
                    onOpen={(docId) => setOpen({ docId, projectId: c.projectId })}
                  />
                ))}
              </div>
            ))}
        </section>
      ))}
      <DocPreviewDialog
        docId={open?.docId ?? null}
        projectId={open?.projectId ?? null}
        query={bundle.query}
        onOpenChange={(next) => !next && setOpen(null)}
      />
    </div>
  );
}

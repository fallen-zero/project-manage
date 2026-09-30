import { Input } from "@/components/ui/input";

export function SearchPage() {
  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-6 py-16">
      <h1 className="text-2xl font-semibold">搜索</h1>
      <Input autoFocus placeholder="搜项目、网址、备注、文档正文…" className="h-12 text-base" disabled />
      <p className="text-sm text-muted-foreground">
        输入框当前为占位：台账字段检索在 M2 接入，文档正文检索在 M3 建索引后接入。
      </p>
    </div>
  );
}

import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { EMPTY_INPUT, type ProjectInput } from "@/types/project";
import type { ParentOption } from "@/lib/project-tree";
import type { Workspace } from "@/types/workspace";

// 中英文逗号、顿号、空格都当分隔符：台账里的标签是人手打的，分隔符不会统一。
function parseTags(raw: string): string[] {
  return [...new Set(raw.split(/[,，、\s]+/).map((t) => t.trim()).filter(Boolean))];
}

const FIELDS: { key: keyof ProjectInput; label: string; placeholder?: string }[] = [
  { key: "code", label: "项目编号", placeholder: "P-2026-001" },
  { key: "manager", label: "项目经理" },
  { key: "customer", label: "客户名称" },
  { key: "contactName", label: "对接联系人" },
  { key: "contactPhone", label: "联系方式" },
  { key: "contractNo", label: "合同编号" },
  { key: "contractPeriod", label: "合同周期", placeholder: "2026-01 ~ 2026-06" },
  { key: "deliveryDeadline", label: "交付截止", placeholder: "2026-06-30" },
];

export function ProjectForm({
  initial,
  statuses,
  saving,
  onSubmit,
  onCancel,
  nameLabel = "项目名称",
  nameRequiredMessage = "项目名称不能为空",
  onChange,
  parentOptions,
  workspaceOptions,
}: {
  initial?: ProjectInput;
  statuses: string[];
  saving: boolean;
  onSubmit: (input: ProjectInput) => Promise<boolean>;
  onCancel: () => void;
  nameLabel?: string;
  nameRequiredMessage?: string;
  onChange?: (input: ProjectInput) => void;
  parentOptions?: ParentOption[];
  workspaceOptions?: Workspace[];
}) {
  const [form, setForm] = useState<ProjectInput>(initial ?? EMPTY_INPUT);
  const [tagsText, setTagsText] = useState((initial ?? EMPTY_INPUT).tags.join("，"));
  const [nameError, setNameError] = useState<string | null>(null);

  const emit = (next: ProjectInput, tags: string) =>
    onChange?.({ ...next, tags: parseTags(tags) });
  const patch = (key: keyof ProjectInput, value: string) => {
    const next = { ...form, [key]: value };
    setForm(next);
    emit(next, tagsText);
  };
  const onTagsChange = (value: string) => {
    setTagsText(value);
    emit(form, value);
  };
  const setParent = (value: string) => {
    const parentId = value === "none" ? null : value;
    const next = { ...form, parentId };
    setForm(next);
    emit(next, tagsText);
  };

  const currentParentId = form.parentId;
  const parentValue =
    currentParentId !== null && parentOptions?.some((o) => o.id === currentParentId)
      ? currentParentId
      : "none";

  // 换区只对「根项目」开放：有父（库里的或本次提交的）就随父走（spec §六.2）。
  const hasParent = form.parentId !== null;
  // 有父时下拉禁用、提交 null；显示值要回到原区，否则会「看着 B、实际留 A」。
  const workspaceValue =
    (hasParent ? initial?.workspaceId : form.workspaceId) ??
    workspaceOptions?.find((w) => w.isActive)?.id ??
    "";
  const workspaceHint = hasParent ? "子项目随父项目一起换区，请编辑它的父项目" : null;

  const submit = async () => {
    if (!form.name.trim()) {
      setNameError(nameRequiredMessage);
      return;
    }
    const parentId =
      parentOptions && form.parentId && !parentOptions.some((o) => o.id === form.parentId)
        ? null
        : form.parentId;
    const workspaceId = hasParent ? null : (form.workspaceId ?? null);
    const ok = await onSubmit({ ...form, parentId, workspaceId, tags: parseTags(tagsText) });
    if (!ok) setNameError("保存失败，详见下方错误提示");
  };

  return (
    <div className="grid gap-3">
      <div className="grid gap-2">
        <Label htmlFor="p-name">
          {nameLabel} <span className="text-destructive">*</span>
        </Label>
        <Input
          id="p-name"
          value={form.name}
          onChange={(e) => {
            patch("name", e.target.value);
            setNameError(null);
          }}
          placeholder="例：某单位运维平台二期"
        />
        {nameError && <p className="text-xs text-destructive">{nameError}</p>}
      </div>

      <div className="grid gap-2">
        <Label>项目状态</Label>
        <Select value={form.status} onValueChange={(v) => patch("status", String(v))}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {statuses.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {parentOptions && (
        <div className="grid gap-2">
          <Label>所属总包</Label>
          <Select
            value={parentValue}
            onValueChange={setParent}
            disabled={parentOptions.length === 0}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">无（顶层项目）</SelectItem>
              {parentOptions.map((o) => (
                <SelectItem key={o.id} value={o.id}>
                  {"— ".repeat(o.depth)}
                  {o.name}（{o.status}）
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {workspaceOptions && workspaceOptions.length > 0 && (
        <div className="grid gap-2">
          <Label>工作区</Label>
          <Select
            value={workspaceValue}
            onValueChange={(v) => patch("workspaceId", String(v))}
            disabled={hasParent || workspaceOptions.length === 0}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {workspaceOptions.map((w) => (
                <SelectItem key={w.id} value={w.id}>
                  {w.name}（{w.projectCount}）
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {workspaceHint && <p className="text-xs text-muted-foreground">{workspaceHint}</p>}
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {FIELDS.map((f) => (
          <div key={f.key} className="grid gap-1.5">
            <Label htmlFor={f.key} className="text-xs text-muted-foreground">
              {f.label}
            </Label>
            <Input
              id={f.key}
              value={(form[f.key] as string | null) ?? ""}
              placeholder={f.placeholder}
              onChange={(e) => patch(f.key, e.target.value)}
            />
          </div>
        ))}
      </div>

      <div className="grid gap-2">
        <Label htmlFor="p-tags" className="text-xs text-muted-foreground">
          自定义标签（逗号或空格分隔）
        </Label>
        <Input
          id="p-tags"
          value={tagsText}
          placeholder="政务，维保中"
          onChange={(e) => onTagsChange(e.target.value)}
        />
      </div>

      <div className="mt-2 flex justify-end gap-2">
        <Button variant="ghost" onClick={onCancel} disabled={saving}>
          取消
        </Button>
        <Button onClick={() => void submit()} disabled={saving}>
          {saving ? "保存中…" : "保存"}
        </Button>
      </div>
    </div>
  );
}

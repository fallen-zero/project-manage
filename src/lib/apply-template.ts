// 套用语义（M12 spec D3）：项目名永不动；模板有值覆盖；模板空/null 不清空；
// status 总覆盖；tags 非空才覆盖。只 import type ⇒ node --test 可直跑。
import type { ProjectInput } from "@/types/project";
import type { ProjectTemplate } from "@/types/project-template";

const filled = (v: string | null): v is string => v !== null && v.trim() !== "";
// ProjectInput 的可空字段是 string | null（types/project.ts），所以 current 也要接 null：
// 模板空/null 时原样保留 null，不会退化成 ""。简报里的签名只写了 string，tsc 会打 8 条 TS2345。
const pick = (current: string | null, next: string | null): string | null =>
  filled(next) ? next : current;

export function applyTemplate(input: ProjectInput, template: ProjectTemplate): ProjectInput {
  return {
    name: input.name,
    parentId: input.parentId,
    // 套模板不是换区：用户先选好的工作区必须留住，否则「先选区、再套模板」会静默丢选择。
    workspaceId: input.workspaceId,
    code: pick(input.code, template.code),
    manager: pick(input.manager, template.manager),
    customer: pick(input.customer, template.customer),
    contactName: pick(input.contactName, template.contactName),
    contactPhone: pick(input.contactPhone, template.contactPhone),
    contractNo: pick(input.contractNo, template.contractNo),
    contractPeriod: pick(input.contractPeriod, template.contractPeriod),
    deliveryDeadline: pick(input.deliveryDeadline, template.deliveryDeadline),
    status: template.status,
    tags: template.tags.length > 0 ? [...template.tags] : [...input.tags],
  };
}

// 台账导出的默认文件名（M11-b spec D8）。只有保存对话框之前需要这个名字，所以它活在前端；
// 后端只收最终 path，不复制同一套清洗规则（一份规则一处活）。
const ILLEGAL = /[\\/:*?"<>|\u0000-\u001F]/g;

export function defaultFileName(projectName: string, format: "xlsx" | "md", today: string): string {
  const stem = Array.from(
    projectName
      .replace(ILLEGAL, "_")
      .trim()
      .replace(/[. ]+$/, ""),
  )
    .slice(0, 60)
    .join("");
  return `台账-${stem || "项目"}-${today}.${format}`;
}

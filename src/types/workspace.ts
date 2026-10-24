// 与 Rust 侧 #[serde(rename_all = "camelCase")] 的字段一一对应（`workspace.rs` 的 `Workspace`）。
// `Option<String>` 序列化成 null，所以这里可空字段一律写 `| null`，不写可选属性；
// `workspace.ts` 四件都是非空的，所以没有 `?`。
export interface Workspace {
  id: string;
  name: string;
  /** 只数未软删的项目（管理弹窗里的显示值）。后端的「非空不许删」守卫**含软删行**，
   *  两个口径不同是刻意的，别把它当删除可行性判据的唯一来源（判据在 `deleteGuard` + 后端）。 */
  projectCount: number;
  /** 这一行是不是当前区。不为它单开第 6 条命令（spec D6 拍的是 5 条）。 */
  isActive: boolean;
}

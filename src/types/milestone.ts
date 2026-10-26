export interface Milestone {
  id: string;
  projectId: string;
  stage: string;
  planDate: string | null;
  note: string;
  done: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface MilestoneInput {
  projectId: string;
  stage: string;
  planDate: string | null;
  note: string | null;
}

export interface MilestoneEdit {
  stage: string;
  planDate: string | null;
  note: string | null;
  done: boolean;
}

// 需求原文那五个词（docs/思维导图.md:35-36）。与 projects.status 那八段是两套词（spec D4）：
// 这一份喂里程碑的下拉，statusOptions()（现成命令）喂项目状态的下拉，**不许互换**。
// lib.rs 的 stage_words_are_identical_in_rust_and_the_frontend_type 钉这三处逐字一致。
export const MILESTONE_STAGES = ["立项", "评审", "联调", "上线", "验收"] as const;

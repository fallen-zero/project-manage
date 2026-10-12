//! 文件监听（M5.5 唯一新机制）。这一层**只置一个脏布尔**，绝不碰 SQLite：
//! notify 回调跑在 notify 内部线程，若在那儿直接写库就绕过了 index_job 的 running 单飞闸门。
//! 真正的追平仍由 ticker 调既有 index_job::start(None, rebuild=false) 走那一整轮（spec D1/D3）。
#![allow(dead_code)] // Task 8 接线（notify + ticker 接进 setup）后回收本豁免

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 「自上次追平之后有没有过文件事件」的唯一位点。D2：不是项目集合，就是一个布尔
/// （index_job::start 只吃单个 Option<String> project_id、targets() 追平轮本就该扫全部）。
pub struct WatchShared {
    pub dirty: AtomicBool,
    /// 「有没有轮在跑」的判据位点。**注意**：`IndexShared.running` 是裸 `AtomicBool` 字段
    /// （Arc 套在 `IndexShared` 外，`index_job.rs:397`），拿不到与它同址的 `Arc<AtomicBool>`。
    /// 故本字段是占位、Task 8 接线时**必须落定**：最懒可行 = ticker 直接持 `Arc<IndexShared>`
    /// 读 `index.running.load()` 传进 `due_for_pass`（`index_job.rs` 零改动），届时删掉本字段。
    /// 见账本 Task 7 的 Ruling。（此处不谎称「已共享同一个 Arc」——那兑现不了。）
    pub running: Arc<AtomicBool>,
}

impl WatchShared {
    pub fn new(running: Arc<AtomicBool>) -> Self {
        Self { dirty: AtomicBool::new(false), running }
    }
}

pub fn mark_dirty(shared: &WatchShared) {
    shared.dirty.store(true, Ordering::SeqCst);
}

/// ticker 的决策：见脏且空闲 ⇒ 清脏并返回 true。
/// running 判定必须在 swap **之前**（spec 第四节①，Step 2 的测试逼出的判据）——
/// 「先起轮后清脏」会让轮跑期间进来的新事件被下一轮重复起，而 running 时消费脏位
/// 会让轮结束后再也追不平这些事件（那是真漏）。
pub fn due_for_pass(shared: &WatchShared) -> bool {
    // running 时**不消费**脏位（否则自动轮跑了几分钟、期间的事件被 swap 清空，轮结束后
    // 这些事件永不触发追平 —— 那是真漏）。空闲时才 swap-and-clear 起轮。
    if shared.running.load(Ordering::SeqCst) {
        return false;
    }
    shared.dirty.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shared_is_not_dirty() {
        let w = WatchShared::new(Arc::new(AtomicBool::new(false)));
        assert!(!due_for_pass(&w), "初值必须未占用（M5 同款钉法）");
    }

    #[test]
    fn mark_dirty_then_due_consumes_the_flag_once() {
        let w = WatchShared::new(Arc::new(AtomicBool::new(false)));
        mark_dirty(&w);
        assert!(due_for_pass(&w), "置脏且空闲 → 该起一轮");
        assert!(!due_for_pass(&w), "起过就该清脏，第二拍不该再起（除非又来事件）");
    }

    #[test]
    fn running_suppresses_pass_even_when_dirty() {
        let w = WatchShared::new(Arc::new(AtomicBool::new(true))); // 有轮在跑
        mark_dirty(&w);
        assert!(!due_for_pass(&w), "running 时不起新轮");
        // running 期间的脏位不能被吞：轮结束后仍要靠它追平一次
        w.running.store(false, Ordering::SeqCst);
        assert!(due_for_pass(&w), "running 期间的脏位不能被吞，空闲后仍要能追平");
    }
}

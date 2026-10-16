//! 一次性目录 + Drop guard（M7 为 LibreOffice 转换造的，M8 起 `backup` 也用，故提到顶层）。
//! 不用 `tempfile`：它在 `[dev-dependencies]`，生产代码拿不到。
//!
//! **每笔操作一个目录**：M7 的教训（spec §三.16）是共用目录/共用 profile 会让抢输的一方零产物，
//! M8 的快照与解包同理 —— 同一轮里两个 scratch 绝不能撞到同一个父目录名上。
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// `prefix` 是 `%TEMP%` 下的分组目录名（调用方给，如 `pfm-m7` / `pfm-m8`），
    /// `tag` 是这一次操作的标签。名字里带 epoch 纳秒：只靠 pid+seq 时，上一轮 Drop 的
    /// `remove_dir_all` 若被外部进程句柄顶住而静默失败，OS 复用同一 pid 会让新目录
    /// 直接端走陈旧目录，「产物存在」判据就在旧产物上通过了。
    pub fn new(prefix: &str, tag: &str) -> AppResult<Scratch> {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir()
            .join(prefix)
            .join(format!("{}-{}-{}-{}", std::process::id(), n, nanos, tag));
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, &e))?;
        Ok(Scratch { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 `legacy_office.rs` 原样搬来的那条：Drop 必须把整棵目录删掉（成功与失败路径都走这一条）。
    #[test]
    fn scratch_dir_disappears_when_it_is_dropped() {
        let s = Scratch::new("pfm-m7-test", "t1").unwrap();
        let dir = s.dir().to_path_buf();
        std::fs::write(dir.join("x.txt"), b"y").unwrap();
        assert!(dir.is_dir(), "new 之后目录必须在");
        drop(s);
        assert!(!dir.exists(), "Drop 必须把临时目录整棵删掉（失败路径也走这一条）");
    }

    /// 前缀必须**真的被用上**：M8 靠这条保证两个模块的一次性目录不落在同一个父目录下，
    /// 也让 `%TEMP%` 里的分组仍按里程碑分得开（M7 spec §三 记的就是这个账）。
    #[test]
    fn scratch_creates_under_the_given_prefix_not_a_hardcoded_one() {
        let a = Scratch::new("pfm-m8-test-a", "one").unwrap();
        let b = Scratch::new("pfm-m8-test-b", "two").unwrap();
        let pa = a.dir().to_path_buf();
        let pb = b.dir().to_path_buf();
        assert!(pa.starts_with(std::env::temp_dir().join("pfm-m8-test-a")), "前缀没进路径：{pa:?}");
        assert!(pb.starts_with(std::env::temp_dir().join("pfm-m8-test-b")), "前缀没进路径：{pb:?}");
        assert_ne!(pa, pb, "同 tag 不同前缀也必须不撞（一次一目录）");
        drop(a);
        drop(b);
        assert!(!pa.exists() && !pb.exists(), "两个 scratch 都要被 Drop 清掉");
    }
}

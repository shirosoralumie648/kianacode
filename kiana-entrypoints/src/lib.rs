//! Kiana 各种本地入口的模块注册表。
//!
//! 入口模块负责参数解析、终端/Web 展示和协议适配，但产品执行仍必须汇聚到
//! `DaemonHost`。这里的 `pub mod` 只暴露模块边界，不代表每个历史入口都属于当前
//! CompanyOS 产品路径；`tui`、旧 `sdk`、旧 `runner` 与兼容命令面应按各自文档中的
//! proof level 使用，不能拿它们的输出替代 ControlPlane/EventLog 证据。
//!
//! 测试辅助模块只在测试构建中提供环境变量锁和恢复工具，避免并发测试互相污染进程级
//! 环境；它不会进入生产二进制，也不参与运行时授权。

pub mod bg;
pub mod budget_presenter;
pub mod cli;
pub mod command_dispatch;
pub mod company_surface_parity;
pub mod company_user_flow;
pub mod extension_projection;
pub mod harness_run;
pub mod init;
pub mod mcp;
pub mod model_failure_presenter;
pub mod notification_cli;
pub mod product_command;
pub mod provider_diagnostics;
pub mod repl;
pub mod runner;
pub mod sandbox;
pub mod sdk;
mod stream_render;
pub mod tty_input;
pub mod tui;
pub mod web;
pub(crate) mod web_inbox;
pub mod web_notifications;
mod web_thread;
mod web_ui;
pub mod workbench;
mod workbench_chat;
pub mod workbench_controller;
pub mod workbench_render;
pub mod workbench_review;

/// 仅供 crate 测试使用的进程级环境变量互斥与作用域恢复工具。
///
/// Rust 测试可能并行运行，而环境变量属于整个进程；多个测试若同时改同一个键，会读到彼此的临时值。
/// 约定测试通过这里的同一把锁读写相关键。此模块只在 test build 存在，不参与生产入口路由或授权。
#[cfg(test)]
pub(crate) mod test_support {
    use std::convert::Infallible;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// 所有遵守该 helper 约定的测试共用的环境变量锁。
    pub(crate) struct EnvLock(Mutex<()>);

    /// 持锁期间的环境变量临时覆盖；离开作用域时恢复进入前的状态。
    pub(crate) struct ScopedEnv<'a> {
        /// 持有锁到 `ScopedEnv` drop 完成，保证恢复旧值时其他 helper 使用者仍被阻塞。
        _guard: MutexGuard<'a, ()>,
        /// 对每个目标键保存原状态；`None` 与原来存在但值为空字符串是两种不同状态。
        values: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvLock {
        /// 仅获取互斥锁，不读取或更改环境变量。
        ///
        /// `PoisonError` 表示先前持锁线程 panic；这里取回仍可用的 guard，并以 `Infallible` 结果保留
        /// 可用 `?` 的调用形式，避免单个测试 panic 后使后续环境变量测试永久无法运行。
        pub(crate) fn lock(&self) -> Result<MutexGuard<'_, ()>, Infallible> {
            Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner))
        }

        /// 持锁并保存一组键的原始 OS 字符串值，返回 RAII 临时环境作用域。
        ///
        /// 锁先于读取快照获取，因此所有使用同一 `EnvLock` 的测试不会在保存/恢复之间插入写入。
        /// 本 helper 不会阻止绕过它直接调用 `std::env::set_var` 的代码。
        pub(crate) fn scoped<'a>(&'a self, keys: &[&'static str]) -> ScopedEnv<'a> {
            let guard = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            let values = keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect();
            ScopedEnv {
                _guard: guard,
                values,
            }
        }
    }

    impl ScopedEnv<'_> {
        /// 在当前锁定作用域中读取原始 OS 值，不假设值一定是合法 UTF-8。
        pub(crate) fn var_os(&self, key: &str) -> Option<OsString> {
            std::env::var_os(key)
        }

        /// 在当前进程里设置临时值；必须通过仍持锁的 `ScopedEnv` 执行，避免测试之间互相覆盖。
        pub(crate) fn set_var(&self, key: &str, value: impl AsRef<OsStr>) {
            std::env::set_var(key, value);
        }
    }

    impl Drop for ScopedEnv<'_> {
        /// 以逆序恢复进入作用域前的值；原来不存在的键会删除，而非留下空字符串。
        ///
        /// `Drop` 在普通离开作用域和 panic 展开时都会运行。该实现先完成恢复，再释放 `_guard`。
        fn drop(&mut self) {
            for (key, value) in self.values.iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// 返回本测试进程内唯一的环境变量互斥锁。
    pub(crate) fn env_lock() -> &'static EnvLock {
        static LOCK: EnvLock = EnvLock(Mutex::new(()));
        &LOCK
    }

    /// 对全局测试锁启动一段指定键的临时覆盖作用域。
    pub(crate) fn scoped_env(keys: &[&'static str]) -> ScopedEnv<'static> {
        env_lock().scoped(keys)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{mpsc, Arc};
        use std::thread;

        /// 覆盖原先已设置与原先不存在的两种恢复路径。
        #[test]
        fn scoped_env_restores_existing_and_missing_values() {
            let key = "KIANA_TEST_SCOPED_ENV_RESTORE";
            std::env::set_var(key, "before");
            let lock = EnvLock(Mutex::new(()));

            {
                let env = lock.scoped(&[key]);
                env.set_var(key, "during");
                assert_eq!(
                    env.var_os(key).as_deref(),
                    Some(std::ffi::OsStr::new("during"))
                );
            }
            assert_eq!(
                std::env::var_os(key).as_deref(),
                Some(std::ffi::OsStr::new("before"))
            );

            std::env::remove_var(key);
            {
                let env = lock.scoped(&[key]);
                env.set_var(key, "temporary");
            }
            assert_eq!(std::env::var_os(key), None);
        }

        /// 确认某测试线程 panic 污染 Mutex 后，helper 仍会回收锁并继续恢复环境值。
        #[test]
        fn scoped_env_recovers_after_lock_poisoning() {
            let key = "KIANA_TEST_SCOPED_ENV_POISON";
            std::env::remove_var(key);
            let lock = Arc::new(EnvLock(Mutex::new(())));
            let poison_lock = Arc::clone(&lock);

            assert!(thread::spawn(move || {
                let _guard = poison_lock.lock().unwrap();
                panic!("poison test environment lock");
            })
            .join()
            .is_err());

            {
                let env = lock.scoped(&[key]);
                env.set_var(key, "recovered");
                assert_eq!(
                    env.var_os(key).as_deref(),
                    Some(std::ffi::OsStr::new("recovered"))
                );
            }
            assert_eq!(std::env::var_os(key), None);
        }

        /// 先让 writer 持锁写临时值，再启动 reader，最后释放 writer；最终读到 baseline 用来检查 RAII 恢复。
        /// ⚠ `reader.is_finished()` 的即时检查不能单独证明 reader 已进入锁等待，阶段边界由同步通道协调。
        #[test]
        fn scoped_env_serializes_environment_readers_and_writers() {
            let key = "KIANA_TEST_SCOPED_ENV_SERIALIZATION";
            std::env::set_var(key, "baseline");
            let lock = Arc::new(EnvLock(Mutex::new(())));
            let writer_lock = Arc::clone(&lock);
            let reader_lock = Arc::clone(&lock);
            let (writer_ready_tx, writer_ready_rx) = mpsc::channel();
            let (release_writer_tx, release_writer_rx) = mpsc::channel();
            let (reader_started_tx, reader_started_rx) = mpsc::channel();

            let writer = thread::spawn(move || {
                let env = writer_lock.scoped(&[key]);
                env.set_var(key, "transient");
                writer_ready_tx.send(()).unwrap();
                release_writer_rx.recv().unwrap();
            });
            writer_ready_rx.recv().unwrap();

            let reader = thread::spawn(move || {
                reader_started_tx.send(()).unwrap();
                let env = reader_lock.scoped(&[key]);
                env.var_os(key)
            });
            reader_started_rx.recv().unwrap();
            assert!(!reader.is_finished());
            release_writer_tx.send(()).unwrap();
            writer.join().unwrap();

            assert_eq!(
                reader.join().unwrap().as_deref(),
                Some(std::ffi::OsStr::new("baseline"))
            );
            std::env::remove_var(key);
        }
    }
}

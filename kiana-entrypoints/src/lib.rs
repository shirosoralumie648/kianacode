pub mod bg;
pub mod cli;
pub mod command_dispatch;
pub mod harness_run;
pub mod init;
pub mod mcp;
pub mod repl;
pub mod runner;
pub mod sandbox;
pub mod sdk;
pub mod tui;
pub mod workbench;

#[cfg(test)]
pub(crate) mod test_support {
    use std::convert::Infallible;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    pub(crate) struct EnvLock(Mutex<()>);

    pub(crate) struct ScopedEnv<'a> {
        _guard: MutexGuard<'a, ()>,
        values: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvLock {
        pub(crate) fn lock(&self) -> Result<MutexGuard<'_, ()>, Infallible> {
            Ok(self.0.lock().unwrap_or_else(PoisonError::into_inner))
        }

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
        pub(crate) fn var_os(&self, key: &str) -> Option<OsString> {
            std::env::var_os(key)
        }

        pub(crate) fn set_var(&self, key: &str, value: impl AsRef<OsStr>) {
            std::env::set_var(key, value);
        }
    }

    impl Drop for ScopedEnv<'_> {
        fn drop(&mut self) {
            for (key, value) in self.values.iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    pub(crate) fn env_lock() -> &'static EnvLock {
        static LOCK: EnvLock = EnvLock(Mutex::new(()));
        &LOCK
    }

    pub(crate) fn scoped_env(keys: &[&'static str]) -> ScopedEnv<'static> {
        env_lock().scoped(keys)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{mpsc, Arc};
        use std::thread;

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

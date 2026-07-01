pub mod bg;
pub mod cli;
pub mod init;
pub mod mcp;
pub mod repl;
pub mod runner;
pub mod sandbox;
pub mod sdk;
pub mod tui;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    pub(crate) fn env_lock() -> &'static Mutex<()> {
        static LOCK: Mutex<()> = Mutex::new(());
        &LOCK
    }
}

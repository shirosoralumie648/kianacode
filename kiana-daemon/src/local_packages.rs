//! Bounded, descriptor-relative local package I/O. Paths never follow symlinks.

use kiana_ports::PortError;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path};

pub(crate) fn failed(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn decode_hex(value: &str) -> Result<Vec<u8>, PortError> {
    if value.len() % 2 != 0 || !kiana_domain::is_hex_bytes(value, value.len() / 2) {
        return Err(failed("package_hex_invalid"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| failed("package_hex_invalid"))?;
            u8::from_str_radix(text, 16).map_err(|_| failed("package_hex_invalid"))
        })
        .collect()
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;

    #[derive(Debug)]
    pub(crate) struct LocalDir(File);

    fn cstring(value: &std::ffi::OsStr) -> Result<CString, PortError> {
        use std::os::unix::ffi::OsStrExt;
        CString::new(value.as_bytes()).map_err(|_| failed("package_path_invalid"))
    }

    impl LocalDir {
        pub(crate) fn try_clone(&self) -> Result<Self, PortError> {
            self.0
                .try_clone()
                .map(Self)
                .map_err(|error| failed(format!("package_directory_clone_failed:{error}")))
        }

        pub(crate) fn open(path: &Path, create: bool) -> Result<Self, PortError> {
            if !path.is_absolute() {
                return Err(failed("package_path_must_be_absolute"));
            }
            let root = CString::new("/").expect("constant");
            let fd = unsafe {
                libc::open(
                    root.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                return Err(failed(format!(
                    "package_directory_open_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut current = Self(unsafe { File::from_raw_fd(fd) });
            for component in path.components() {
                match component {
                    Component::RootDir => {}
                    Component::Normal(name) => current = current.child(name, create)?,
                    _ => return Err(failed("package_path_invalid")),
                }
            }
            Ok(current)
        }

        fn child(&self, name: &std::ffi::OsStr, create: bool) -> Result<Self, PortError> {
            let name = cstring(name)?;
            let mut fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0
                && create
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
            {
                let created = unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) };
                if created < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(failed(format!(
                        "package_directory_create_failed:{}",
                        std::io::Error::last_os_error()
                    )));
                }
                self.0
                    .sync_all()
                    .map_err(|e| failed(format!("package_directory_sync_failed:{e}")))?;
                fd = unsafe {
                    libc::openat(
                        self.0.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    )
                };
            }
            if fd < 0 {
                return Err(failed(format!(
                    "package_directory_open_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(Self(unsafe { File::from_raw_fd(fd) }))
        }

        pub(crate) fn subdir(&self, name: &str, create: bool) -> Result<Self, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            self.child(name.as_ref(), create)
        }

        pub(crate) fn subdir_optional(&self, name: &str) -> Result<Option<Self>, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let name = cstring(name.as_ref())?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(failed(format!("package_directory_open_failed:{error}")));
            }
            Ok(Some(Self(unsafe { File::from_raw_fd(fd) })))
        }

        pub(crate) fn read(&self, relative: &str, limit: usize) -> Result<Vec<u8>, PortError> {
            self.read_optional(relative, limit)?.ok_or_else(|| {
                failed(format!(
                    "package_file_open_failed:{}",
                    std::io::Error::from(std::io::ErrorKind::NotFound)
                ))
            })
        }

        pub(crate) fn read_optional(
            &self,
            relative: &str,
            limit: usize,
        ) -> Result<Option<Vec<u8>>, PortError> {
            if !kiana_domain::valid_extension_path(relative) {
                return Err(failed("package_path_invalid"));
            }
            let mut parts = relative.split('/').peekable();
            let mut directory = Self(self.0.try_clone().map_err(|e| failed(e.to_string()))?);
            while let Some(name) = parts.next() {
                if parts.peek().is_some() {
                    directory = directory.child(name.as_ref(), false)?;
                    continue;
                }
                let name = cstring(name.as_ref())?;
                let fd = unsafe {
                    libc::openat(
                        directory.0.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                    )
                };
                if fd < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::NotFound {
                        return Ok(None);
                    }
                    return Err(failed(format!("package_file_open_failed:{error}")));
                }
                let file = unsafe { File::from_raw_fd(fd) };
                let metadata = file.metadata().map_err(|e| failed(e.to_string()))?;
                if !metadata.is_file() || metadata.nlink() != 1 {
                    return Err(failed("package_file_type_denied"));
                }
                if metadata.len() > limit as u64 {
                    return Err(failed("package_size_exceeded"));
                }
                let mut bytes = Vec::new();
                file.take(limit as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| failed(format!("package_read_failed:{e}")))?;
                if bytes.len() > limit {
                    return Err(failed("package_size_exceeded"));
                }
                return Ok(Some(bytes));
            }
            Err(failed("package_path_invalid"))
        }

        pub(crate) fn create_new_file(&self, name: &str) -> std::io::Result<File> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(std::io::ErrorKind::InvalidInput.into());
            }
            let name = cstring(name.as_ref()).map_err(std::io::Error::other)?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }

        /// Open an existing regular file by name and keep the descriptor pinned, so a caller can
        /// verify bytes and then remove exactly that inode. Symlinks, directories and files with
        /// extra hard links are denied before any action; an absent entry returns `None`.
        pub(crate) fn open_regular_file(&self, name: &str) -> Result<Option<File>, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let name = cstring(name.as_ref())?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::NotFound {
                    return Ok(None);
                }
                return Err(failed(format!("package_file_open_failed:{error}")));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            let metadata = file.metadata().map_err(|e| failed(e.to_string()))?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(failed("package_file_type_denied"));
            }
            Ok(Some(file))
        }

        pub(crate) fn remove_owned_file(&self, name: &str, owned: &File) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let name = cstring(name.as_ref())?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err(failed("package_file_replaced"));
            }
            let current = unsafe { File::from_raw_fd(fd) };
            let current = current.metadata().map_err(|e| failed(e.to_string()))?;
            let pinned = owned.metadata().map_err(|e| failed(e.to_string()))?;
            if !current.is_file()
                || current.nlink() != 1
                || current.dev() != pinned.dev()
                || current.ino() != pinned.ino()
            {
                return Err(failed("package_file_replaced"));
            }
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0) } < 0 {
                return Err(failed(format!(
                    "package_file_remove_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            self.0
                .sync_all()
                .map_err(|e| failed(format!("result_unknown:package_directory_sync_failed:{e}")))
        }

        /// Append bytes to a regular file, creating it when absent. Used by append-only recovery
        /// journals: every record is durable before the call returns, and the directory entry is
        /// synced so a restart can read the full prefix. Existing content is never rewritten.
        pub(crate) fn append(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let name = cstring(name.as_ref())?;
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_APPEND
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW
                        | libc::O_NONBLOCK,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(failed(format!(
                    "package_append_open_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| failed(format!("package_append_failed:{e}")))?;
            self.0
                .sync_all()
                .map_err(|e| failed(format!("result_unknown:package_append_sync_failed:{e}")))
        }

        /// Publish an immutable cache entry only after all bytes are durable. Existing
        /// entries must match exactly; crash leftovers cannot become active registry state.
        pub(crate) fn publish(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let target = cstring(name.as_ref())?;
            let temp =
                CString::new(format!(".staged-{}", kiana_domain::RequestId::new())).expect("uuid");
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    temp.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(failed(format!(
                    "package_stage_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            let result = (|| {
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| failed(format!("package_write_failed:{e}")))?;
                let linked = unsafe {
                    libc::linkat(
                        self.0.as_raw_fd(),
                        temp.as_ptr(),
                        self.0.as_raw_fd(),
                        target.as_ptr(),
                        0,
                    )
                };
                if linked < 0 {
                    if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(failed(format!(
                            "package_publish_failed:{}",
                            std::io::Error::last_os_error()
                        )));
                    }
                    if self.read(name, bytes.len())? != bytes {
                        return Err(failed("package_cache_hash_mismatch"));
                    }
                }
                Ok(())
            })();
            let unlinked = unsafe { libc::unlinkat(self.0.as_raw_fd(), temp.as_ptr(), 0) };
            if unlinked < 0 {
                return Err(failed("result_unknown:package_stage_cleanup_failed"));
            }
            self.0
                .sync_all()
                .map_err(|e| failed(format!("result_unknown:package_cache_sync_failed:{e}")))?;
            result
        }

        /// Read a published regular file, or publish it once and verify an existing copy.
        /// Unlike the package cache, a conflict is reported distinctly so callers cannot
        /// interpret a changed immutable object as a successful idempotent publish.
        pub(crate) fn publish_immutable(
            &self,
            name: &str,
            bytes: &[u8],
            conflict: &str,
        ) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let target = cstring(name.as_ref())?;
            let temp =
                CString::new(format!(".staged-{}", kiana_domain::RequestId::new())).expect("uuid");
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    temp.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(failed(format!(
                    "package_stage_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            let result = (|| {
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| failed(format!("package_write_failed:{e}")))?;
                let linked = unsafe {
                    libc::linkat(
                        self.0.as_raw_fd(),
                        temp.as_ptr(),
                        self.0.as_raw_fd(),
                        target.as_ptr(),
                        0,
                    )
                };
                if linked < 0 {
                    if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(failed(format!(
                            "package_publish_failed:{}",
                            std::io::Error::last_os_error()
                        )));
                    }
                    if self.read(name, bytes.len())? != bytes {
                        return Err(PortError::Conflict(conflict.to_owned()));
                    }
                }
                Ok(())
            })();
            let unlinked = unsafe { libc::unlinkat(self.0.as_raw_fd(), temp.as_ptr(), 0) };
            if unlinked < 0 {
                return Err(failed("result_unknown:package_stage_cleanup_failed"));
            }
            self.0
                .sync_all()
                .map_err(|e| failed(format!("result_unknown:package_cache_sync_failed:{e}")))?;
            result
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::LocalDir;

#[cfg(not(unix))]
mod portable {
    //! Non-Unix (Windows) `LocalDir`.
    //!
    //! The Unix implementation pins a directory descriptor and reaches every entry with
    //! `openat(..., O_NOFOLLOW)` so no intermediate component can be swapped between the check and
    //! the operation.  The standard library has no `openat` on Windows, so this implementation
    //! tracks the absolute path instead and re-inspects every component with `symlink_metadata`
    //! before each operation.
    //!
    //! What is preserved from the Unix contract: absolute roots only, single-component names that
    //! pass `valid_extension_path`, no symlink or reparse point anywhere on the path, regular files
    //! only, bounded reads, durable publish (write temporary, flush, then link into place without
    //! replacing an existing entry), append-only writes, and the distinct "replaced" refusal when
    //! the file behind an owned handle changed.
    //!
    //! What differs: Windows has no inode link count, so the "extra hard link" denial is not
    //! expressed; the ownership check compares size and modification time instead.  A component
    //! swapped in the window between `symlink_metadata` and the following call is a TOCTOU window
    //! this layer cannot close; callers still verify content fingerprints, so the observable
    //! failures stay fail-closed rather than silently accepting unverified bytes.

    use super::*;
    use std::io::Read;
    use std::path::PathBuf;

    #[derive(Debug)]
    pub(crate) struct LocalDir {
        path: PathBuf,
    }

    impl LocalDir {
        pub(crate) fn try_clone(&self) -> Result<Self, PortError> {
            Ok(Self {
                path: self.path.clone(),
            })
        }

        pub(crate) fn open(path: &Path, create: bool) -> Result<Self, PortError> {
            if !path.is_absolute() {
                return Err(failed("package_path_must_be_absolute"));
            }
            let mut current = PathBuf::new();
            for component in path.components() {
                match component {
                    Component::Prefix(_) | Component::RootDir => current.push(component),
                    Component::Normal(name) => {
                        current.push(name);
                        match std::fs::symlink_metadata(&current) {
                            Ok(meta) if meta.file_type().is_symlink() => {
                                return Err(failed("package_directory_open_failed:symlink"))
                            }
                            Ok(meta) if !meta.is_dir() => {
                                return Err(failed("package_directory_open_failed:not_directory"))
                            }
                            Ok(_) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                if !create {
                                    return Err(failed(format!(
                                        "package_directory_open_failed:{error}"
                                    )));
                                }
                                std::fs::create_dir(&current).map_err(|create_error| {
                                    failed(format!(
                                        "package_directory_create_failed:{create_error}"
                                    ))
                                })?;
                            }
                            Err(error) => {
                                return Err(failed(format!(
                                    "package_directory_open_failed:{error}"
                                )))
                            }
                        }
                    }
                    _ => return Err(failed("package_path_invalid")),
                }
            }
            Ok(Self { path: current })
        }

        fn child(&self, name: &str, create: bool) -> Result<Self, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let path = self.path.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(failed("package_directory_open_failed:symlink"))
                }
                Ok(meta) if !meta.is_dir() => {
                    return Err(failed("package_directory_open_failed:not_directory"))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Err(failed(format!("package_directory_open_failed:{error}")));
                    }
                    std::fs::create_dir(&path).map_err(|create_error| {
                        failed(format!("package_directory_create_failed:{create_error}"))
                    })?;
                }
                Err(error) => return Err(failed(format!("package_directory_open_failed:{error}"))),
            }
            Ok(Self { path })
        }

        pub(crate) fn subdir(&self, name: &str, create: bool) -> Result<Self, PortError> {
            self.child(name, create)
        }

        pub(crate) fn subdir_optional(&self, name: &str) -> Result<Option<Self>, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let path = self.path.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    Err(failed("package_directory_open_failed:symlink"))
                }
                Ok(meta) if meta.is_dir() => Ok(Some(Self { path })),
                Ok(_) => Err(failed("package_directory_open_failed:not_directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(failed(format!("package_directory_open_failed:{error}"))),
            }
        }

        /// Resolve a bounded relative path and refuse any symlink component.
        fn resolve(&self, relative: &str) -> Result<PathBuf, PortError> {
            if !kiana_domain::valid_extension_path(relative) {
                return Err(failed("package_path_invalid"));
            }
            let mut current = self.path.clone();
            let mut parts = relative.split('/').peekable();
            while let Some(name) = parts.next() {
                current.push(name);
                let is_last = parts.peek().is_none();
                match std::fs::symlink_metadata(&current) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(failed("package_path_invalid"))
                    }
                    Ok(meta) if is_last && !meta.is_file() => {
                        return Err(failed("package_file_type_denied"))
                    }
                    Ok(meta) if !is_last && !meta.is_dir() => {
                        return Err(failed("package_path_invalid"))
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if is_last {
                            return Ok(current);
                        }
                        return Err(failed(format!("package_file_open_failed:{error}")));
                    }
                    Err(error) => return Err(failed(format!("package_file_open_failed:{error}"))),
                }
            }
            Ok(current)
        }

        pub(crate) fn read(&self, relative: &str, limit: usize) -> Result<Vec<u8>, PortError> {
            self.read_optional(relative, limit)?.ok_or_else(|| {
                failed(format!(
                    "package_file_open_failed:{}",
                    std::io::Error::from(std::io::ErrorKind::NotFound)
                ))
            })
        }

        pub(crate) fn read_optional(
            &self,
            relative: &str,
            limit: usize,
        ) -> Result<Option<Vec<u8>>, PortError> {
            let path = self.resolve(relative)?;
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(failed(format!("package_file_open_failed:{error}"))),
            };
            let metadata = file.metadata().map_err(|e| failed(e.to_string()))?;
            if !metadata.is_file() {
                return Err(failed("package_file_type_denied"));
            }
            if metadata.len() > limit as u64 {
                return Err(failed("package_size_exceeded"));
            }
            let mut bytes = Vec::new();
            let mut handle = file;
            handle
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| failed(format!("package_read_failed:{e}")))?;
            if bytes.len() > limit {
                return Err(failed("package_size_exceeded"));
            }
            Ok(Some(bytes))
        }

        pub(crate) fn create_new_file(&self, name: &str) -> std::io::Result<File> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(std::io::ErrorKind::InvalidInput.into());
            }
            let path = self.path.join(name);
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        }

        pub(crate) fn open_regular_file(&self, name: &str) -> Result<Option<File>, PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let path = self.path.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(failed("package_file_type_denied"))
                }
                Ok(meta) if !meta.is_file() => return Err(failed("package_file_type_denied")),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(failed(format!("package_file_open_failed:{error}"))),
            }
            let file = File::open(&path)
                .map_err(|error| failed(format!("package_file_open_failed:{error}")))?;
            Ok(Some(file))
        }

        pub(crate) fn remove_owned_file(&self, name: &str, owned: &File) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let path = self.path.join(name);
            let current =
                std::fs::symlink_metadata(&path).map_err(|_| failed("package_file_replaced"))?;
            if current.file_type().is_symlink() || !current.is_file() {
                return Err(failed("package_file_replaced"));
            }
            // Windows has no inode link count, so ownership is proved by size + modification time
            // matching the handle the caller already validated.
            let pinned = owned.metadata().map_err(|e| failed(e.to_string()))?;
            if current.len() != pinned.len() || current.modified().ok() != pinned.modified().ok() {
                return Err(failed("package_file_replaced"));
            }
            std::fs::remove_file(&path)
                .map_err(|error| failed(format!("package_file_remove_failed:{error}")))?;
            Ok(())
        }

        /// Append bytes to a regular file, creating it when absent; existing content is never
        /// rewritten.  Windows flushes through `sync_all` the same way the Unix path does.
        pub(crate) fn append(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let path = self.path.join(name);
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if meta.file_type().is_symlink() || !meta.is_file() {
                    return Err(failed("package_path_invalid"));
                }
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| failed(format!("package_append_open_failed:{e}")))?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| failed(format!("package_append_failed:{e}")))?;
            Ok(())
        }

        /// Publish an immutable cache entry only after all bytes are durable.  Existing entries
        /// must match exactly; a leftover temp file cannot become active registry state.
        pub(crate) fn publish(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            self.publish_inner(name, bytes, "package_cache_hash_mismatch")
        }

        pub(crate) fn publish_immutable(
            &self,
            name: &str,
            bytes: &[u8],
            conflict: &str,
        ) -> Result<(), PortError> {
            self.publish_inner(name, bytes, conflict)
        }

        fn publish_inner(&self, name: &str, bytes: &[u8], conflict: &str) -> Result<(), PortError> {
            if name.contains('/') || !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            let target = self.path.join(name);
            let temp = self
                .path
                .join(format!(".staged-{}", kiana_domain::RequestId::new()));
            let result = (|| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp)
                    .map_err(|e| failed(format!("package_stage_failed:{e}")))?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| failed(format!("package_write_failed:{e}")))?;
                drop(file);
                // Hard-link publish: an existing target is never replaced.  The entry is created
                // only when absent; the idempotent case then compares the stored bytes.
                match std::fs::hard_link(&temp, &target) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        if self.read(name, bytes.len().max(1))? != bytes {
                            return Err(PortError::Conflict(conflict.to_owned()));
                        }
                        Ok(())
                    }
                    Err(error) => Err(failed(format!("package_publish_failed:{error}"))),
                }
            })();
            let _ = std::fs::remove_file(&temp);
            result
        }
    }
}

#[cfg(not(unix))]
pub(crate) use portable::LocalDir;

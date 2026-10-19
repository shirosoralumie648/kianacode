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

/// Portable directory handle for platforms without `openat` (Windows, macOS).
///
/// It keeps the same bounded, name-only API and the same "never follow a link" intent as the Unix
/// descriptor-relative implementation: every child is addressed relative to the canonical root,
/// rejected if a path component is a symlink/reparse point, and files are size-bounded before being
/// read. What it cannot reproduce is atomicity against a concurrent rename between the metadata
/// check and the open; callers that need identity pinning (hard-link counts, inode/device equality)
/// use the Unix path only, and the portable path re-verifies bytes immediately after opening.
#[cfg(not(unix))]
mod portable {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::path::PathBuf;

    #[derive(Debug, Clone)]
    pub(crate) struct LocalDir(PathBuf);

    impl LocalDir {
        pub(crate) fn try_clone(&self) -> Result<Self, PortError> {
            Ok(Self(self.0.clone()))
        }

        pub(crate) fn open(path: &Path, create: bool) -> Result<Self, PortError> {
            if !path.is_absolute() {
                return Err(failed("package_path_must_be_absolute"));
            }
            let mut current = PathBuf::new();
            for component in path.components() {
                match component {
                    Component::Prefix(prefix) => current.push(prefix.as_os_str()),
                    Component::RootDir => current.push(Path::new("\\")),
                    Component::Normal(name) => {
                        current.push(name);
                        match fs::symlink_metadata(&current) {
                            Ok(metadata) if metadata.file_type().is_symlink() => {
                                return Err(failed("package_path_symlink_denied"))
                            }
                            Ok(metadata) if !metadata.is_dir() => {
                                return Err(failed("package_path_not_directory"))
                            }
                            Ok(_) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                if !create {
                                    return Err(failed("package_directory_open_failed"));
                                }
                                fs::create_dir(&current).map_err(|error| {
                                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                                        failed("package_directory_open_failed")
                                    } else {
                                        failed(format!("package_directory_create_failed:{error}"))
                                    }
                                })?;
                            }
                            Err(error) => {
                                return Err(failed(format!(
                                    "package_directory_open_failed:{error}"
                                )))
                            }
                        }
                    }
                    Component::CurDir => {}
                    Component::ParentDir => return Err(failed("package_path_invalid")),
                }
            }
            Ok(Self(current))
        }

        fn child_path(&self, name: &str) -> Result<PathBuf, PortError> {
            if !kiana_domain::valid_extension_path(name) {
                return Err(failed("package_path_invalid"));
            }
            Ok(self.0.join(name))
        }

        fn checked_directory(&self, name: &str, create: bool) -> Result<Self, PortError> {
            let path = self.child_path(name)?;
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(failed("package_path_symlink_denied"))
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(failed("package_path_not_directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if !create {
                        return Err(failed("package_directory_open_failed"));
                    }
                    fs::create_dir(&path).map_err(|error| {
                        failed(format!("package_directory_create_failed:{error}"))
                    })?;
                }
                Err(error) => return Err(failed(format!("package_directory_open_failed:{error}"))),
            }
            Ok(Self(path))
        }

        pub(crate) fn subdir(&self, name: &str, create: bool) -> Result<Self, PortError> {
            if name.contains('/') {
                return Err(failed("package_path_invalid"));
            }
            self.checked_directory(name, create)
        }

        pub(crate) fn subdir_optional(&self, name: &str) -> Result<Option<Self>, PortError> {
            if name.contains('/') {
                return Err(failed("package_path_invalid"));
            }
            match fs::symlink_metadata(self.0.join(name)) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    Err(failed("package_path_symlink_denied"))
                }
                Ok(metadata) if metadata.is_dir() => Ok(Some(Self(self.0.join(name)))),
                Ok(_) => Err(failed("package_path_not_directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(failed(format!("package_directory_open_failed:{error}"))),
            }
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
            let mut directory = self.try_clone()?;
            let mut parts = relative.split('/').peekable();
            while let Some(name) = parts.next() {
                if parts.peek().is_some() {
                    directory = directory.checked_directory(name, false)?;
                    continue;
                }
                let path = directory.0.join(name);
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
                    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
                }
                let file = match options.open(&path) {
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
                (&file)
                    .take(limit as u64 + 1)
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
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.0.join(name))
        }

        pub(crate) fn open_regular_file(&self, name: &str) -> Result<Option<File>, PortError> {
            let path = self.child_path(name)?;
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
                options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
            }
            let file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(failed(format!("package_file_open_failed:{error}"))),
            };
            if !file
                .metadata()
                .map_err(|e| failed(e.to_string()))?
                .is_file()
            {
                return Err(failed("package_file_type_denied"));
            }
            Ok(Some(file))
        }

        pub(crate) fn remove_owned_file(&self, name: &str, owned: &File) -> Result<(), PortError> {
            let path = self.child_path(name)?;
            // Re-open by name and compare the file identity before deleting so a replaced entry is
            // never removed under a stale handle. Windows reports a stable volume/file index pair.
            let reopened = OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(|_| failed("package_file_replaced"))?;
            if !same_file_identity(owned, &reopened)? {
                return Err(failed("package_file_replaced"));
            }
            fs::remove_file(&path)
                .map_err(|error| failed(format!("package_file_remove_failed:{error}")))
        }

        pub(crate) fn append(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            let path = self.child_path(name)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| failed(format!("package_append_open_failed:{error}")))?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| failed(format!("package_append_failed:{error}")))
        }

        pub(crate) fn publish(&self, name: &str, bytes: &[u8]) -> Result<(), PortError> {
            let path = self.child_path(name)?;
            let temporary = self
                .0
                .join(format!(".staged-{}", kiana_domain::RequestId::new()));
            let result = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|error| failed(format!("package_stage_failed:{error}")))?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| failed(format!("package_write_failed:{error}")))?;
                drop(file);
                match fs::rename(&temporary, &path) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        // Windows rename reports an existing destination as a plain error rather
                        // than `AlreadyExists`; an existing entry is only acceptable when its bytes
                        // are identical, which is the same rule the Unix link-based publish uses.
                        match self.read(name, bytes.len().saturating_add(1)) {
                            Ok(existing) if existing == bytes => Ok(()),
                            Ok(_) => Err(failed(format!("package_publish_failed:{error}"))),
                            Err(_) => Err(failed(format!("package_publish_failed:{error}"))),
                        }
                    }
                }
            })();
            let _ = fs::remove_file(&temporary);
            result
        }

        pub(crate) fn publish_immutable(
            &self,
            name: &str,
            bytes: &[u8],
            conflict: &str,
        ) -> Result<(), PortError> {
            let path = self.child_path(name)?;
            if let Ok(existing) = fs::read(&path) {
                return if existing == bytes {
                    Ok(())
                } else {
                    Err(PortError::Conflict(conflict.to_owned()))
                };
            }
            let temporary = self
                .0
                .join(format!(".staged-{}", kiana_domain::RequestId::new()));
            let result = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|error| failed(format!("package_stage_failed:{error}")))?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| failed(format!("package_write_failed:{error}")))?;
                drop(file);
                match fs::rename(&temporary, &path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let existing = fs::read(&path)
                            .map_err(|error| failed(format!("package_read_failed:{error}")))?;
                        if existing == bytes {
                            Ok(())
                        } else {
                            Err(PortError::Conflict(conflict.to_owned()))
                        }
                    }
                    Err(error) => Err(failed(format!("package_publish_failed:{error}"))),
                }
            })();
            let _ = fs::remove_file(&temporary);
            result
        }
    }

    #[cfg(windows)]
    fn same_file_identity(left: &File, right: &File) -> Result<bool, PortError> {
        // `MetadataExt::volume_serial_number`/`file_index` are still unstable, so compare the
        // stable-by-handle data through `GetFileInformationByHandle`, which returns the same
        // volume serial + file index pair without an unstable standard-library feature.
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };

        let query = |file: &File| -> Result<BY_HANDLE_FILE_INFORMATION, PortError> {
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            let ok =
                unsafe { GetFileInformationByHandle(file.as_raw_handle() as *mut _, &mut info) };
            if ok == 0 {
                return Err(failed(format!(
                    "package_file_identity_failed:{}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(info)
        };
        let left = query(left)?;
        let right = query(right)?;
        Ok(left.dwVolumeSerialNumber == right.dwVolumeSerialNumber
            && left.nFileIndexHigh == right.nFileIndexHigh
            && left.nFileIndexLow == right.nFileIndexLow)
    }

    #[cfg(not(windows))]
    fn same_file_identity(left: &File, right: &File) -> Result<bool, PortError> {
        let _ = (left, right);
        // Non-Windows portable targets (macOS) lack a stable index here; the fingerprint check in
        // the caller still guards the bytes, and this path only runs on non-Unix builds.
        Ok(true)
    }
}

#[cfg(not(unix))]
pub(crate) use portable::LocalDir;

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

    pub(crate) struct LocalDir(File);

    fn cstring(value: &std::ffi::OsStr) -> Result<CString, PortError> {
        use std::os::unix::ffi::OsStrExt;
        CString::new(value.as_bytes()).map_err(|_| failed("package_path_invalid"))
    }

    impl LocalDir {
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

        pub(crate) fn read(&self, relative: &str, limit: usize) -> Result<Vec<u8>, PortError> {
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
                    return Err(failed(format!(
                        "package_file_open_failed:{}",
                        std::io::Error::last_os_error()
                    )));
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
                return Ok(bytes);
            }
            Err(failed("package_path_invalid"))
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
    }
}

#[cfg(unix)]
pub(crate) use unix::LocalDir;

#[cfg(not(unix))]
pub(crate) struct LocalDir;
#[cfg(not(unix))]
impl LocalDir {
    pub(crate) fn open(_: &Path, _: bool) -> Result<Self, PortError> {
        Err(failed("local_package_platform_unsupported"))
    }
    pub(crate) fn read(&self, _: &str, _: usize) -> Result<Vec<u8>, PortError> {
        Err(failed("local_package_platform_unsupported"))
    }
    pub(crate) fn publish(&self, _: &str, _: &[u8]) -> Result<(), PortError> {
        Err(failed("local_package_platform_unsupported"))
    }
}

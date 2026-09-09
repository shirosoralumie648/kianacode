use super::*;

pub(crate) fn write_symposium_artifacts(
    project_root: &str,
    meeting: &Symposium,
    decision: &DecisionRecord,
    packet: Option<&WorkPacket>,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("symposium_artifact_write_failed");
    }
    let decision_path = Path::new(meeting.decision_path());
    let decision_json =
        serde_json::to_string_pretty(decision).map_err(|_| "symposium_artifact_write_failed")?;
    write_project_artifact(
        root,
        decision_path,
        decision_json.as_bytes(),
        "symposium_artifact_write_failed",
    )?;
    if let Some(packet) = packet {
        let packet_json =
            serde_json::to_string_pretty(packet).map_err(|_| "symposium_artifact_write_failed")?;
        write_project_artifact(
            root,
            Path::new(WORK_PACKET_PATH),
            packet_json.as_bytes(),
            "symposium_artifact_write_failed",
        )?;
    }
    Ok(())
}

pub(crate) fn read_review_artifact(project_root: &str) -> Result<ReviewPacket, &'static str> {
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Err("close_project_not_found");
    }
    let raw = read_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        "close_review_not_found",
    )?;
    let review: ReviewPacket = serde_json::from_str(&raw).map_err(|_| "close_review_invalid")?;
    review.validate().map_err(|_| "close_review_invalid")?;
    Ok(review)
}

pub(crate) fn write_closing_artifact(
    project_root: &str,
    receipt: &ClosingReceipt,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("closing_artifact_write_failed");
    }
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "closing_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::CLOSING_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "closing_artifact_write_failed",
    )?;
    let mut learned = String::from("# Closing lessons\n\n");
    learned.push_str("The Builder output passed independent Review and was accepted by Closing.\n");
    if !receipt.files_verified.is_empty() {
        learned.push_str("\nVerified files:\n");
        for file in &receipt.files_verified {
            learned.push_str("- ");
            learned.push_str(file);
            learned.push('\n');
        }
    }
    write_project_artifact(
        root,
        Path::new("lessons/LEARNED.md"),
        learned.as_bytes(),
        "closing_artifact_write_failed",
    )
}

pub(crate) fn read_merge_artifact(project_root: &str) -> Result<MergeReceipt, &'static str> {
    let root = Path::new(project_root);
    let raw = read_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        "close_merge_receipt_not_found",
    )?;
    let merge: MergeReceipt =
        serde_json::from_str(&raw).map_err(|_| "close_merge_receipt_invalid")?;
    merge
        .validate()
        .map_err(|_| "close_merge_receipt_invalid")?;
    Ok(merge)
}

pub(crate) fn write_merge_artifact(
    project_root: &str,
    receipt: &MergeReceipt,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("merge_artifact_write_failed");
    }
    let receipt_json =
        serde_json::to_string_pretty(receipt).map_err(|_| "merge_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(kiana_domain::MERGE_RECEIPT_PATH),
        receipt_json.as_bytes(),
        "merge_artifact_write_failed",
    )
}

pub(crate) fn write_review_artifact(
    project_root: &str,
    packet: &ReviewPacket,
) -> Result<(), &'static str> {
    let root = Path::new(project_root);
    if project_root.trim().is_empty() || !root.is_dir() {
        return Err("review_artifact_write_failed");
    }
    let packet_json =
        serde_json::to_string_pretty(packet).map_err(|_| "review_artifact_write_failed")?;
    write_project_artifact(
        root,
        Path::new(REVIEW_PACKET_PATH),
        packet_json.as_bytes(),
        "review_artifact_write_failed",
    )?;
    Ok(())
}

const ARTIFACT_TEMP_ATTEMPTS: usize = 16;
#[cfg(target_os = "linux")]
const LINUX_RENAME_NOREPLACE: u32 = 1;
#[cfg(target_os = "linux")]
const LINUX_RENAME_EXCHANGE: u32 = 2;

#[cfg(target_os = "linux")]
fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    read_project_artifact_linux(root, relative, error)
}

#[cfg(not(target_os = "linux"))]
fn read_project_artifact(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    let path = confined_artifact_path(root, relative, error)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|_| error)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;
    Ok(raw)
}

#[cfg(target_os = "linux")]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    prepare_project_artifact_linux(root, relative, contents, error)?.commit()
}

#[cfg(not(target_os = "linux"))]
fn write_project_artifact(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<(), &'static str> {
    let path = confined_artifact_path(root, relative, error)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| error)?;
    }
    confined_artifact_path(root, relative, error)?;
    let (temporary, mut file) = create_artifact_temp_sibling(&path, error)?;
    let write_result = (|| {
        file.write_all(contents).map_err(|_| error)?;
        file.flush().map_err(|_| error)?;
        file.sync_all().map_err(|_| error)
    })();
    drop(file);
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        return write_result;
    }
    if confined_artifact_path(root, relative, error).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn confined_artifact_path(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<PathBuf, &'static str> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(error);
    }
    let path = root.join(relative);
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(error);
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(error),
            Ok(_) => {}
            Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error),
        }
    }
    Ok(path)
}

#[cfg(not(target_os = "linux"))]
fn create_artifact_temp_sibling(
    path: &Path,
    error: &'static str,
) -> Result<(PathBuf, File), &'static str> {
    let name = path.file_name().ok_or(error)?.to_string_lossy();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        let temporary = path.with_file_name(format!(
            ".{name}.kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(io_error) if io_error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(error),
        }
    }
    Err(error)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxArtifactTargetState {
    Missing,
    Regular {
        device: u64,
        inode: u64,
        mode: u32,
        links: u64,
        size: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    },
}

#[cfg(target_os = "linux")]
struct LinuxArtifactLocation {
    root_path: PathBuf,
    root: File,
    parent: File,
    parent_components: Vec<OsString>,
    target: CString,
}

#[cfg(target_os = "linux")]
impl LinuxArtifactLocation {
    fn open(
        root_path: &Path,
        relative: &Path,
        create_parents: bool,
        error: &'static str,
    ) -> Result<Self, &'static str> {
        let mut components = validated_artifact_components(relative, error)?;
        let target = components.pop().ok_or(error)?;
        let root = linux_open_artifact_root(root_path).map_err(|_| error)?;
        let mut parent = root.try_clone().map_err(|_| error)?;
        for component in &components {
            parent =
                linux_open_artifact_directory_at(parent.as_raw_fd(), component, create_parents)
                    .map_err(|_| error)?;
        }
        Ok(Self {
            root_path: root_path.to_path_buf(),
            root,
            parent,
            parent_components: components,
            target: linux_artifact_cstring(&target).map_err(|_| error)?,
        })
    }

    fn is_current(&self) -> bool {
        let Ok(root) = linux_open_artifact_root(&self.root_path) else {
            return false;
        };
        if !linux_same_directory(&self.root, &root) {
            return false;
        }
        let mut parent = root;
        for component in &self.parent_components {
            let Ok(next) = linux_open_artifact_directory_at(parent.as_raw_fd(), component, false)
            else {
                return false;
            };
            parent = next;
        }
        linux_same_directory(&self.parent, &parent)
    }
}

#[cfg(target_os = "linux")]
struct PreparedLinuxArtifactWrite {
    location: LinuxArtifactLocation,
    temporary: CString,
    expected: LinuxArtifactTargetState,
    temporary_present: bool,
    error: &'static str,
}

#[cfg(target_os = "linux")]
impl PreparedLinuxArtifactWrite {
    fn commit(mut self) -> Result<(), &'static str> {
        if !self.location.is_current() {
            return Err(self.error);
        }
        let current =
            linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.location.target)
                .map_err(|_| self.error)?;
        if current != self.expected {
            return Err(self.error);
        }

        match self.expected {
            LinuxArtifactTargetState::Missing => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_NOREPLACE,
                )
                .map_err(|_| self.error)?;
                self.temporary_present = false;
            }
            LinuxArtifactTargetState::Regular { .. } => {
                linux_artifact_renameat2(
                    self.location.parent.as_raw_fd(),
                    &self.temporary,
                    &self.location.target,
                    LINUX_RENAME_EXCHANGE,
                )
                .map_err(|_| self.error)?;
                let displaced =
                    linux_artifact_target_state(self.location.parent.as_raw_fd(), &self.temporary);
                if !matches!(displaced, Ok(state) if state == self.expected) {
                    if linux_artifact_renameat2(
                        self.location.parent.as_raw_fd(),
                        &self.temporary,
                        &self.location.target,
                        LINUX_RENAME_EXCHANGE,
                    )
                    .is_err()
                    {
                        self.temporary_present = false;
                    }
                    return Err(self.error);
                }
                linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary)
                    .map_err(|_| self.error)?;
                self.temporary_present = false;
            }
        }
        let _ = linux_artifact_sync_directory(self.location.parent.as_raw_fd());
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for PreparedLinuxArtifactWrite {
    fn drop(&mut self) {
        if self.temporary_present {
            let _ = linux_artifact_unlinkat(self.location.parent.as_raw_fd(), &self.temporary);
        }
    }
}

#[cfg(target_os = "linux")]
fn read_project_artifact_linux(
    root: &Path,
    relative: &Path,
    error: &'static str,
) -> Result<String, &'static str> {
    let location = LinuxArtifactLocation::open(root, relative, false, error)?;
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;
    if matches!(expected, LinuxArtifactTargetState::Missing) {
        return Err(error);
    }
    let fd = unsafe {
        libc::openat(
            location.parent.as_raw_fd(),
            location.target.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error);
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected {
        return Err(error);
    }
    let mut raw = String::new();
    file.read_to_string(&mut raw).map_err(|_| error)?;
    if linux_artifact_file_state(&file).map_err(|_| error)? != expected
        || !location.is_current()
        || linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
            .map_err(|_| error)?
            != expected
    {
        return Err(error);
    }
    Ok(raw)
}

#[cfg(target_os = "linux")]
fn prepare_project_artifact_linux(
    root: &Path,
    relative: &Path,
    contents: &[u8],
    error: &'static str,
) -> Result<PreparedLinuxArtifactWrite, &'static str> {
    let location = LinuxArtifactLocation::open(root, relative, true, error)?;
    let expected = linux_artifact_target_state(location.parent.as_raw_fd(), &location.target)
        .map_err(|_| error)?;
    let (temporary, mut file) =
        linux_create_artifact_temp(location.parent.as_raw_fd()).map_err(|_| error)?;
    let result = (|| {
        file.write_all(contents).map_err(|_| error)?;
        file.flush().map_err(|_| error)?;
        file.sync_all().map_err(|_| error)
    })();
    drop(file);
    if let Err(write_error) = result {
        let _ = linux_artifact_unlinkat(location.parent.as_raw_fd(), &temporary);
        return Err(write_error);
    }
    Ok(PreparedLinuxArtifactWrite {
        location,
        temporary,
        expected,
        temporary_present: true,
        error,
    })
}

#[cfg(target_os = "linux")]
fn validated_artifact_components(
    relative: &Path,
    error: &'static str,
) -> Result<Vec<OsString>, &'static str> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(error);
    }
    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(error),
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_open_artifact_root(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(target_os = "linux")]
fn linux_open_artifact_directory_at(
    parent: RawFd,
    component: &std::ffi::OsStr,
    create: bool,
) -> std::io::Result<File> {
    let component = linux_artifact_cstring(component)?;
    let open = || unsafe {
        libc::openat(
            parent,
            component.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let mut fd = open();
    if fd < 0 && create && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        let mkdir = unsafe { libc::mkdirat(parent, component.as_ptr(), 0o755) };
        if mkdir != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(std::io::Error::last_os_error());
        }
        fd = open();
    }
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_target_state(
    parent: RawFd,
    name: &CString,
) -> std::io::Result<LinuxArtifactTargetState> {
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ENOENT) {
            Ok(LinuxArtifactTargetState::Missing)
        } else {
            Err(error)
        };
    }
    let file = unsafe { File::from_raw_fd(fd) };
    linux_artifact_file_state(&file)
}

#[cfg(target_os = "linux")]
fn linux_artifact_file_state(file: &File) -> std::io::Result<LinuxArtifactTargetState> {
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "artifact target is not a single-link regular file",
        ));
    }
    Ok(LinuxArtifactTargetState::Regular {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        links: metadata.nlink(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

#[cfg(target_os = "linux")]
fn linux_same_directory(left: &File, right: &File) -> bool {
    match (left.metadata(), right.metadata()) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn linux_create_artifact_temp(parent: RawFd) -> std::io::Result<(CString, File)> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..ARTIFACT_TEMP_ATTEMPTS {
        let name = CString::new(format!(
            ".kiana-artifact-{}-{stamp}-{attempt}",
            std::process::id()
        ))
        .expect("generated artifact name cannot contain NUL");
        let fd = unsafe {
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            return Ok((name, unsafe { File::from_raw_fd(fd) }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "artifact temporary name attempts exhausted",
    ))
}

#[cfg(target_os = "linux")]
fn linux_artifact_renameat2(
    parent: RawFd,
    source: &CString,
    target: &CString,
    flags: u32,
) -> std::io::Result<()> {
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent,
            source.as_ptr(),
            parent,
            target.as_ptr(),
            flags,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_unlinkat(parent: RawFd, name: &CString) -> std::io::Result<()> {
    if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_sync_directory(parent: RawFd) -> std::io::Result<()> {
    if unsafe { libc::fsync(parent) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn linux_artifact_cstring(value: &std::ffi::OsStr) -> std::io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact path contains NUL",
        )
    })
}
#[cfg(all(test, target_os = "linux"))]
mod artifact_path_tests {
    use super::{prepare_project_artifact_linux, write_project_artifact};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_nanos();
        for attempt in 0..16 {
            let root = std::env::temp_dir().join(format!(
                "kiana-core-artifact-{label}-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            if fs::create_dir(&root).is_ok() {
                return root;
            }
        }
        panic!("could not create temporary artifact test root");
    }

    #[test]
    fn prepared_artifact_write_rejects_parent_rename_before_commit() {
        let root = temporary_root("parent-rename");
        let outside = temporary_root("parent-rename-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::rename(root.join("gate"), root.join("gate-before-rename")).unwrap();
        symlink(&outside, root.join("gate")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert!(!outside.join("REVIEW.json").exists());
        assert!(!root.join("gate-before-rename/REVIEW.json").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_final_symlink_replacement_before_commit() {
        let root = temporary_root("target-symlink");
        let outside = temporary_root("target-symlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        symlink(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );
        assert!(fs::symlink_metadata(root.join("gate/REVIEW.json"))
            .unwrap()
            .file_type()
            .is_symlink());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn prepared_artifact_write_rejects_target_version_replacement_before_commit() {
        let root = temporary_root("target-version");
        fs::create_dir(root.join("gate")).unwrap();
        let target = root.join("gate/REVIEW.json");
        fs::write(&target, "original").unwrap();
        let prepared = prepare_project_artifact_linux(
            &root,
            Path::new("gate/REVIEW.json"),
            b"replacement",
            "artifact_write_failed",
        )
        .unwrap();

        fs::write(&target, "changed after prepare").unwrap();

        assert_eq!(prepared.commit(), Err("artifact_write_failed"));
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "changed after prepare"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn artifact_write_rejects_hardlinked_target_without_mutating_source() {
        let root = temporary_root("hardlink");
        let outside = temporary_root("hardlink-outside");
        fs::create_dir(root.join("gate")).unwrap();
        let outside_target = outside.join("outside-target");
        fs::write(&outside_target, "outside sentinel").unwrap();
        fs::hard_link(&outside_target, root.join("gate/REVIEW.json")).unwrap();

        assert_eq!(
            write_project_artifact(
                &root,
                Path::new("gate/REVIEW.json"),
                b"replacement",
                "artifact_write_failed",
            ),
            Err("artifact_write_failed")
        );
        assert_eq!(
            fs::read_to_string(&outside_target).unwrap(),
            "outside sentinel"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}

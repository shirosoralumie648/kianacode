//! BQ-22 local-only provider invoice file import.
//!
//! This adapter reads a bounded local JSON document from an operator-chosen root and hands the
//! bytes to the pure [`kiana_domain::parse_provider_invoice_document`] parser.  It is the only
//! place in the workspace that touches the filesystem for invoice import.
//!
//! # Boundaries
//! - **No network.** There is no HTTP client, socket or URL handling here.  A provider export
//!   must be saved locally first; a network adapter is explicitly out of scope.
//! - **Not a model-visible tool.** The harness never sees this module: it is not registered in the
//!   capability broker or in the runner's tool schemas, so a model transcript cannot be decoded
//!   into an invoice fact.  Callers are operator/entrypoint surfaces.
//! - **Fail closed.** Paths are relative to an absolute root and may not contain `..`, absolute
//!   prefixes or symlink escapes; only regular files are read; the size bound is enforced while
//!   reading (not only from metadata).  Parse failures keep the domain's stable
//!   `provider_invoice_document_*` code.
//! - **No durable idempotency.** Reading a file twice produces the same facts; this adapter does
//!   not remember which files it already imported.  `ProviderInvoiceLedger` fences same-process
//!   replay only; an EventLog-backed `cost.invoice_imported` store remains out of scope.

use kiana_domain::{
    parse_provider_invoice_document, ParsedProviderInvoiceDocument,
    MAX_PROVIDER_INVOICE_DOCUMENT_BYTES,
};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Hard cap applied while reading the file, one byte above the parser limit so an oversized file
/// is detected instead of being silently truncated.
const MAX_PROVIDER_INVOICE_FILE_BYTES: u64 = MAX_PROVIDER_INVOICE_DOCUMENT_BYTES as u64;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
/// Stable refusal codes for the local file adapter.
///
/// `InvalidDocument` carries the domain parser's own `provider_invoice_document_*` code so the
/// caller can distinguish a bad file from a bad path or an oversized file without string
/// matching on OS messages.
pub enum LocalProviderInvoiceImportError {
    /// The configured root is not an absolute directory.
    #[error("provider_invoice_import_root_invalid")]
    RootInvalid,
    /// The relative path escapes the root, is absolute, or is not a regular file.
    #[error("provider_invoice_import_path_invalid")]
    PathInvalid,
    /// The file could not be opened or read.
    #[error("provider_invoice_import_read_failed")]
    ReadFailed,
    /// The file exceeds [`MAX_PROVIDER_INVOICE_DOCUMENT_BYTES`].
    #[error("provider_invoice_import_too_large")]
    TooLarge,
    /// The bytes are not a valid invoice document; the inner value is the domain code.
    #[error("provider_invoice_import_document_invalid:{0}")]
    InvalidDocument(String),
}

/// Reads provider invoice documents from one absolute, operator-chosen directory.
///
/// The root is never inferred from the workspace or `.kiana`; callers must name it explicitly.
#[derive(Clone, Debug)]
pub struct LocalProviderInvoiceImporter {
    root: PathBuf,
}

impl LocalProviderInvoiceImporter {
    /// Open an importer rooted at an absolute existing directory.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LocalProviderInvoiceImportError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(LocalProviderInvoiceImportError::RootInvalid);
        }
        let metadata =
            std::fs::metadata(&root).map_err(|_| LocalProviderInvoiceImportError::RootInvalid)?;
        if !metadata.is_dir() {
            return Err(LocalProviderInvoiceImportError::RootInvalid);
        }
        Ok(Self { root })
    }

    /// Root directory this importer is bound to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read and parse one invoice document named relative to the root.
    ///
    /// The path must consist only of normal components (no `..`, no drive prefix, no absolute
    /// path).  Both the root and the file are canonicalized and the file must stay under the
    /// root, so a symlink inside the root cannot redirect the read elsewhere.
    pub fn import(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<ParsedProviderInvoiceDocument, LocalProviderInvoiceImportError> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(LocalProviderInvoiceImportError::PathInvalid);
        }
        let root = std::fs::canonicalize(&self.root)
            .map_err(|_| LocalProviderInvoiceImportError::RootInvalid)?;
        let file_path = std::fs::canonicalize(root.join(relative))
            .map_err(|_| LocalProviderInvoiceImportError::ReadFailed)?;
        if !file_path.starts_with(&root) {
            return Err(LocalProviderInvoiceImportError::PathInvalid);
        }
        let metadata = std::fs::metadata(&file_path)
            .map_err(|_| LocalProviderInvoiceImportError::ReadFailed)?;
        if !metadata.is_file() {
            return Err(LocalProviderInvoiceImportError::PathInvalid);
        }
        if metadata.len() > MAX_PROVIDER_INVOICE_FILE_BYTES {
            return Err(LocalProviderInvoiceImportError::TooLarge);
        }

        let mut file =
            File::open(&file_path).map_err(|_| LocalProviderInvoiceImportError::ReadFailed)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.by_ref()
            .take(MAX_PROVIDER_INVOICE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| LocalProviderInvoiceImportError::ReadFailed)?;
        if bytes.len() as u64 > MAX_PROVIDER_INVOICE_FILE_BYTES {
            return Err(LocalProviderInvoiceImportError::TooLarge);
        }

        parse_provider_invoice_document(&bytes)
            .map_err(LocalProviderInvoiceImportError::InvalidDocument)
    }
}

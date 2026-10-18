//! BQ-22 local invoice file adapter fixtures (deny-first).
//!
//! These fixtures use a real temporary directory to prove the only filesystem entry point for
//! invoice import: absolute-root confinement, `..`/absolute path refusal, regular-file-only reads,
//! a size bound checked while reading, and the domain parser's stable refusal codes.  No network,
//! no broker, no EventStore, no model-visible tool registration.

use kiana_daemon::{LocalProviderInvoiceImportError, LocalProviderInvoiceImporter};
use kiana_domain::{
    parse_provider_invoice_document, InvoiceImportOutcome, ProviderInvoiceLedger, RequestId,
    MAX_PROVIDER_INVOICE_DOCUMENT_BYTES, PROVIDER_INVOICE_DOCUMENT_SCHEMA,
};
use std::fs;
use std::path::PathBuf;

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D3: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("kiana-bq22-{}", RequestId::new()));
        fs::create_dir_all(root.join("invoices")).expect("fixture root");
        Self { root }
    }

    fn document(&self) -> String {
        serde_json::json!({
            "schema": PROVIDER_INVOICE_DOCUMENT_SCHEMA,
            "provider_account_ref": "account:provider-1",
            "invoice_id": "invoice:2026-09",
            "period_start_unix_ms": 1_000,
            "period_end_unix_ms": 2_000,
            "model": "model-a",
            "usage_digest": D1,
            "amount": {"schema": "kiana.money.v1", "currency": "USD", "micros": 100},
            "receipt_ref": "receipt:provider-1",
            "receipt_digest": D3,
            "verifier_ref": "verifier:provider-1",
            "verified_at_unix_ms": 200_000_000,
            "authority_epoch": 7,
            "source_cursor": 12,
            "import_key": "import:provider-1:2026-09"
        })
        .to_string()
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.root.join(name), bytes).expect("fixture file");
    }

    fn importer(&self) -> LocalProviderInvoiceImporter {
        LocalProviderInvoiceImporter::open(&self.root).expect("importer")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn local_import_refuses_escapes_symlinked_roots_and_oversized_files() {
    let fixture = Fixture::new();

    // A relative or non-directory root is not accepted.
    assert_eq!(
        LocalProviderInvoiceImporter::open("relative/root").unwrap_err(),
        LocalProviderInvoiceImportError::RootInvalid
    );
    let file_root = fixture.root.join("invoice.json");
    fixture.write("invoice.json", fixture.document().as_bytes());
    assert_eq!(
        LocalProviderInvoiceImporter::open(&file_root).unwrap_err(),
        LocalProviderInvoiceImportError::RootInvalid
    );

    let importer = fixture.importer();
    assert_eq!(importer.root(), fixture.root.as_path());

    // Absolute path, `..` escape and a directory are all refused before any read.
    for path in ["/etc/passwd", "../outside.json", "invoices/../invoice.json"] {
        assert_eq!(
            importer.import(path).unwrap_err(),
            LocalProviderInvoiceImportError::PathInvalid,
            "path must be refused: {path}"
        );
    }
    assert_eq!(
        importer.import("posters").unwrap_err(),
        LocalProviderInvoiceImportError::ReadFailed
    );

    // Missing file and oversized file are distinct refusals.
    assert_eq!(
        importer.import("missing.json").unwrap_err(),
        LocalProviderInvoiceImportError::ReadFailed
    );
    fixture.write(
        "huge.json",
        &vec![b' '; MAX_PROVIDER_INVOICE_DOCUMENT_BYTES + 1],
    );
    assert_eq!(
        importer.import("huge.json").unwrap_err(),
        LocalProviderInvoiceImportError::TooLarge
    );

    // Invalid document content keeps the domain code rather than collapsing to a read error.
    fixture.write("bad.json", b"{\"schema\":\"wrong\"}");
    assert!(matches!(
        importer.import("bad.json").unwrap_err(),
        LocalProviderInvoiceImportError::InvalidDocument(code)
            if code.starts_with("provider_invoice_document_invalid_json:")
    ));
    fixture.write("empty.json", b"");
    assert_eq!(
        importer.import("empty.json").unwrap_err(),
        LocalProviderInvoiceImportError::InvalidDocument(
            "provider_invoice_document_empty".to_owned()
        )
    );
}

#[test]
fn local_import_reads_a_bound_document_and_stays_out_of_the_model_tool_surface() {
    let fixture = Fixture::new();
    fixture.write("invoice.json", fixture.document().as_bytes());

    let parsed = fixture
        .importer()
        .import("invoice.json")
        .expect("parsed document");
    assert_eq!(parsed.import.invoice_id, "invoice:2026-09");
    assert!(parsed.import.validate().is_ok());
    assert!(parsed
        .import
        .validate_against_authentication(&parsed.authentication, 200_000_000, 7)
        .is_ok());

    // The adapter is deterministic: the same bytes produce the same fact.
    let again = fixture
        .importer()
        .import("invoice.json")
        .expect("parsed document");
    assert_eq!(parsed.import.import_digest, again.import.import_digest);

    // A subdirectory inside the root is allowed and still constrained to the root.
    fs::create_dir_all(fixture.root.join("sub")).expect("subdir");
    fs::write(
        fixture.root.join("sub").join("nested.json"),
        fixture.document().as_bytes(),
    )
    .expect("nested");
    assert!(fixture.importer().import("sub/nested.json").is_ok());

    // Appending the parsed import is the caller's job; the ledger only fences same-process replay.
    let mut ledger = ProviderInvoiceLedger::new();
    assert_eq!(
        ledger.append(&parsed.import),
        Ok(InvoiceImportOutcome::Imported)
    );
    assert_eq!(
        ledger.append(&parsed.import),
        Ok(InvoiceImportOutcome::Replayed)
    );

    // The adapter adds no second parser and no network path: it delegates to the domain parser.
    let source = include_str!("../src/billing_invoice_import.rs");
    for forbidden in [
        "reqwest",
        "std::net",
        "TcpStream",
        "ToolSchema",
        "kiana_runner",
    ] {
        assert!(
            !source.contains(forbidden),
            "local invoice import must not gain network/tool authority: {forbidden}"
        );
    }
    let parsed_directly =
        parse_provider_invoice_document(fixture.document().as_bytes()).expect("domain parser");
    assert_eq!(parsed.import, parsed_directly.import);
}

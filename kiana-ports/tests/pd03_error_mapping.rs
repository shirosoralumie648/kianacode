use kiana_domain::StorageErrorClass;
use kiana_ports::PortError;

#[test]
fn port_errors_map_to_stable_storage_classes_without_success_fallback() {
    let values = [
        (
            PortError::Unavailable("offline".to_owned()),
            StorageErrorClass::Unavailable,
        ),
        (
            PortError::Conflict("revision".to_owned()),
            StorageErrorClass::Conflict,
        ),
        (
            PortError::Failed("checksum".to_owned()),
            StorageErrorClass::Corrupt,
        ),
        (
            PortError::Failed("result_unknown".to_owned()),
            StorageErrorClass::ResultUnknown,
        ),
        (
            PortError::Failed("empty".to_owned()),
            StorageErrorClass::Empty,
        ),
        (
            PortError::Failed("other".to_owned()),
            StorageErrorClass::Unknown,
        ),
    ];
    for (error, expected) in values {
        assert_eq!(error.storage_class(), expected);
    }
}

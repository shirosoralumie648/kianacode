//! ID 契约注册表。
//!
//! 本模块登记 domain 中公开 ID 类型的唯一 owner 与线协议形状。登记不改变任何现有
//! serde 表示；测试把注册表与真实类型的 JSON 往返行为锁在一起。

/// ID 在 JSON 线协议中的基础形态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdWireShape {
    /// UUID 型 ID，线协议上表现为 JSON 字符串。
    Uuid,
    /// 非 UUID 字符串型 ID，线协议上表现为 JSON 字符串。
    String,
}

/// 一个公开 ID 类型的唯一契约。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdContract {
    /// Rust 类型名，例如 `RequestId`。
    pub type_name: &'static str,
    /// 定义该 canonical 类型的 crate。
    pub owner_crate: &'static str,
    /// 线协议字段名，例如 `request_id`。
    pub wire_name: &'static str,
    /// 线协议基础形态。
    pub wire_shape: IdWireShape,
}

/// `kiana-domain` 当前公开的全部 ID 契约。
pub const ID_CONTRACTS: &[IdContract] = &[
    IdContract {
        type_name: "RequestId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "request_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "RunId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "run_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "TurnId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "turn_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "CellId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "cell_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "EventId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "event_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ExecutionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "execution_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "InvocationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "invocation_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ApprovalId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "approval_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ArtifactId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "artifact_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ReceiptId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "receipt_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "OrganizationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "organization_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "ProjectId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "project_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "TemplateId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "template_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SpawnPlanId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "spawn_plan_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "BudgetLeaseId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "budget_lease_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "CapabilityGrantId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "capability_grant_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SupervisionLeaseId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "supervision_lease_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "DelegationId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "delegation_id",
        wire_shape: IdWireShape::Uuid,
    },
    IdContract {
        type_name: "SessionId",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "session_id",
        wire_shape: IdWireShape::String,
    },
    IdContract {
        type_name: "WorkFingerprint",
        owner_crate: env!("CARGO_PKG_NAME"),
        wire_name: "work_fingerprint",
        wire_shape: IdWireShape::String,
    },
];

#[cfg(test)]
mod tests {
    use super::{IdContract, IdWireShape, ID_CONTRACTS};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::collections::HashSet;
    use std::fmt::Debug;

    const WORKSPACE_MANIFEST: &str = include_str!("../../Cargo.toml");
    const IDS_SOURCE: &str = include_str!("ids.rs");

    trait IdRoundTripSample: Serialize + DeserializeOwned + PartialEq + Debug {
        const WIRE_SHAPE: IdWireShape;

        fn sample() -> Self;
    }

    macro_rules! impl_uuid_samples {
        ($($id:ident),+ $(,)?) => {
            $(
                impl IdRoundTripSample for crate::$id {
                    const WIRE_SHAPE: IdWireShape = IdWireShape::Uuid;

                    fn sample() -> Self {
                        Self::new()
                    }
                }
            )+
        };
    }

    impl_uuid_samples!(
        RequestId,
        RunId,
        TurnId,
        CellId,
        EventId,
        ExecutionId,
        InvocationId,
        ApprovalId,
        ArtifactId,
        ReceiptId,
        OrganizationId,
        ProjectId,
        TemplateId,
        SpawnPlanId,
        BudgetLeaseId,
        CapabilityGrantId,
        SupervisionLeaseId,
        DelegationId,
    );

    impl IdRoundTripSample for crate::SessionId {
        const WIRE_SHAPE: IdWireShape = IdWireShape::String;

        fn sample() -> Self {
            Self::new("session-round-trip")
        }
    }

    impl IdRoundTripSample for crate::WorkFingerprint {
        const WIRE_SHAPE: IdWireShape = IdWireShape::String;

        fn sample() -> Self {
            Self::from_parts("objective", &[], "partition", "output", "policy")
                .expect("valid work fingerprint sample")
        }
    }

    fn contract_for(type_name: &str) -> &'static IdContract {
        ID_CONTRACTS
            .iter()
            .find(|contract| contract.type_name == type_name)
            .unwrap_or_else(|| panic!("missing ID contract for {type_name}"))
    }

    macro_rules! round_trip {
        ($id:ident) => {
            #[allow(non_snake_case)]
            mod $id {
                use super::{contract_for, IdRoundTripSample, IdWireShape};
                use crate::$id;

                #[test]
                fn round_trip_preserves_wire_shape() {
                    let contract = contract_for(stringify!($id));
                    assert_eq!(contract.type_name, stringify!($id));
                    assert_eq!(contract.wire_shape, <$id as IdRoundTripSample>::WIRE_SHAPE);

                    let value = <$id as IdRoundTripSample>::sample();
                    let encoded = serde_json::to_string(&value).expect("serialize ID");
                    let json: serde_json::Value =
                        serde_json::from_str(&encoded).expect("parse serialized ID");
                    let wire_value = json
                        .as_str()
                        .expect("ID wire representation must be a JSON string");
                    let measured_wire_shape = if uuid::Uuid::parse_str(wire_value).is_ok() {
                        IdWireShape::Uuid
                    } else {
                        IdWireShape::String
                    };
                    assert_eq!(contract.wire_shape, measured_wire_shape);

                    let decoded: $id =
                        serde_json::from_str(&encoded).expect("deserialize round trip");
                    assert_eq!(decoded, value);
                }
            }
        };
    }

    macro_rules! register_round_trip_tests {
        ($($id:ident),+ $(,)?) => {
            const ROUND_TRIP_TYPES: &[&str] = &[$(stringify!($id)),+];

            $(round_trip!($id);)+

            #[test]
            fn id_contract_count_matches_registered_types() {
                assert_eq!(ID_CONTRACTS.len(), ROUND_TRIP_TYPES.len());
                for type_name in ROUND_TRIP_TYPES {
                    assert!(
                        ID_CONTRACTS
                            .iter()
                            .any(|contract| contract.type_name == *type_name),
                        "ID_CONTRACTS is missing {type_name}"
                    );
                }
            }
        };
    }

    register_round_trip_tests!(
        RequestId,
        RunId,
        TurnId,
        CellId,
        EventId,
        ExecutionId,
        InvocationId,
        ApprovalId,
        ArtifactId,
        ReceiptId,
        OrganizationId,
        ProjectId,
        TemplateId,
        SpawnPlanId,
        BudgetLeaseId,
        CapabilityGrantId,
        SupervisionLeaseId,
        DelegationId,
        SessionId,
        WorkFingerprint,
    );

    #[test]
    fn every_public_type_has_one_owner_and_a_conversion_test() {
        let workspace_crates = workspace_crate_names();
        let mut type_names: HashSet<&str> = HashSet::new();
        let mut wire_names: HashSet<&str> = HashSet::new();

        for contract in ID_CONTRACTS {
            assert!(
                type_names.insert(contract.type_name),
                "duplicate ID type_name: {}",
                contract.type_name
            );
            assert!(
                wire_names.insert(contract.wire_name),
                "duplicate ID wire_name: {}",
                contract.wire_name
            );
            assert!(
                workspace_crates.contains(&contract.owner_crate),
                "unknown owner crate: {}",
                contract.owner_crate
            );
            assert!(
                ROUND_TRIP_TYPES.contains(&contract.type_name),
                "missing conversion test for: {}",
                contract.type_name
            );
            assert!(
                !contract.wire_name.is_empty(),
                "empty wire_name for: {}",
                contract.type_name
            );
        }
    }

    #[test]
    fn every_id_type_definition_is_registered() {
        let mut declared_types: Vec<&str> = IDS_SOURCE
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("uuid_id!(") {
                    return rest.strip_suffix(");").map(str::trim);
                }
                if let Some(rest) = line.strip_prefix("pub struct ") {
                    let type_name = rest.split(['(', '{']).next()?.trim();
                    if type_name.ends_with("Id") || type_name == "WorkFingerprint" {
                        return Some(type_name);
                    }
                }
                None
            })
            .collect();
        let mut registered_types: Vec<&str> = ID_CONTRACTS
            .iter()
            .map(|contract| contract.type_name)
            .collect();

        declared_types.sort_unstable();
        registered_types.sort_unstable();
        assert_eq!(declared_types, registered_types);
    }

    fn workspace_crate_names() -> Vec<&'static str> {
        let (_, members) = WORKSPACE_MANIFEST
            .split_once("members = [")
            .expect("workspace members declaration");
        let (members, _) = members
            .split_once(']')
            .expect("workspace members terminator");

        members
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let name = line.strip_suffix(',')?.trim();
                name.strip_prefix('"')?.strip_suffix('"')
            })
            .collect()
    }
}

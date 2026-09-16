use kiana_domain::{enforce_path_containment, enforce_root_containment};
use std::path::Path;

#[test]
fn path_containment_is_shared_by_every_side_effecting_tool() {
    let allow = vec!["src".to_owned()];
    assert_eq!(
        enforce_path_containment(&allow, "src\\lib.rs").unwrap(),
        "src/lib.rs"
    );
    assert!(enforce_path_containment(&allow, "../outside").is_err());
    assert!(enforce_path_containment(&allow, "secrets/key").is_err());
    assert!(enforce_path_containment(&[".".to_owned()], ".kiana/state").is_ok());

    let root = Path::new("/workspace/project");
    assert!(enforce_root_containment(root, Path::new("/workspace/project/src")).is_ok());
    assert!(enforce_root_containment(root, Path::new("/workspace/project-old")).is_err());
}

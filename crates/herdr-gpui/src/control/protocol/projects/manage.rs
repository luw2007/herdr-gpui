#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::{Call, Caller, Request};

#[test]
fn manage_has_a_strict_argument_free_wire_contract() {
    let request = Request::Project(Call {
        caller: Caller {
            window: Some(7),
            ..Caller::default()
        },
        op: ProjectOp::Manage {},
    });
    let wire = serde_json::to_string(&request).unwrap();
    assert!(wire.contains(r#""op":{"op":"manage"}"#));
    assert_eq!(serde_json::from_str::<Request>(&wire).unwrap(), request);
    assert!((ProjectOp::Manage {}).validate().is_ok());
    for extra in [r#""project":"alpha""#, r#""force":true"#] {
        let wire =
            format!(r#"{{"method":"project","caller":{{}},"op":{{"op":"manage",{extra}}}}}"#);
        assert!(serde_json::from_str::<Request>(&wire).is_err());
    }
}

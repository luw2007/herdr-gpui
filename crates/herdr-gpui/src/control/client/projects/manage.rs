use super::*;

#[test]
fn manage_maps_to_the_native_manager_request() {
    assert_eq!(operation(ProjectCommand::Manage), ProjectOp::Manage {});
}

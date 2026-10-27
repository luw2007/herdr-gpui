#[test]
fn project_help_explains_manager_scope_and_targeting() {
    let help = super::family("project");
    for required in [
        "project manage [--window ID] [--json]",
        "even with no projects or sessions",
        "including offline",
        "absolute path",
        "record associations",
        "delete server folders",
        "not that the user saved an edit",
    ] {
        assert!(
            help.contains(required),
            "missing manager guidance: {required}"
        );
    }
}

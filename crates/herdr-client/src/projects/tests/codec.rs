use super::*;
use serde_json::{Value, json};

const ID_A: &str = "11111111-1111-4111-8111-111111111111";
const ID_B: &str = "22222222-2222-4222-8222-222222222222";

fn populated() -> Registry {
    let mut registry = Registry::new();
    let app = registry
        .create_project("app", Some(key("git@github.com:o/app.git")))
        .unwrap();
    let task = registry.create_project("ABC-1", None).unwrap();
    registry
        .bind(device("ssh:box"), "/srv/abc", &task.id)
        .unwrap();
    registry
        .bind(device("local"), "/tasks/abc", &task.id)
        .unwrap();
    registry.assign(session("local", "main"), &app.id).unwrap();
    registry
        .assign(session("ssh:box", "dx-wt-abc"), &task.id)
        .unwrap();
    registry
}

fn valid() -> Value {
    json!({
        "version": 1,
        "projects": [
            {"id": ID_A, "name": "app", "repo": "github.com/o/app"},
            {"id": ID_B, "name": "task"},
        ],
        "bindings": [{"device": "local", "path": "/t", "project": ID_B}],
        "assignments": [{"device": "local", "session": "main", "project": ID_A}],
    })
}

type Edit = fn(&mut Value);
type Check = fn(&ProjectError) -> bool;

fn decode(value: &Value) -> Result<Registry, ProjectError> {
    Registry::from_json(&serde_json::to_vec(value).unwrap())
}

#[test]
fn a_registry_round_trips_and_serializes_deterministically() {
    let registry = populated();
    let bytes = registry.to_json().unwrap();
    assert_eq!(Registry::from_json(&bytes).unwrap(), registry);
    assert_eq!(registry.to_json().unwrap(), bytes);
    // The same content built in another order yields identical bytes once ids match.
    let again = Registry::from_json(&bytes).unwrap();
    assert_eq!(again.to_json().unwrap(), bytes);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 1);
    let devices: Vec<&str> = value["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["device"].as_str().unwrap())
        .collect();
    assert_eq!(devices, ["local", "ssh:box"]);
    assert_eq!(
        Registry::new().to_json().unwrap(),
        Registry::new().to_json().unwrap()
    );
    assert_eq!(
        Registry::from_json(&Registry::new().to_json().unwrap()).unwrap(),
        Registry::new()
    );
}

#[test]
fn a_valid_document_decodes() {
    let registry = decode(&valid()).unwrap();
    assert_eq!(registry.projects().len(), 2);
    assert_eq!(
        registry.resolve(&session("local", "main"), &[]).project,
        Some(ProjectId::parse(ID_A).unwrap())
    );
}

#[test]
fn unknown_fields_versions_and_malformed_input_are_rejected() {
    for mutate in [
        |v: &mut Value| v["extra"] = json!(1),
        |v: &mut Value| v["projects"][0]["colour"] = json!("red"),
        |v: &mut Value| v["bindings"][0]["note"] = json!(""),
        |v: &mut Value| v["assignments"][0]["note"] = json!(""),
        |v: &mut Value| {
            v.as_object_mut().unwrap().remove("bindings");
        },
    ] {
        let mut doc = valid();
        mutate(&mut doc);
        assert!(
            matches!(decode(&doc), Err(ProjectError::Schema(_))),
            "{doc}"
        );
    }
    let mut doc = valid();
    doc["version"] = json!(3);
    assert!(matches!(
        decode(&doc),
        Err(ProjectError::UnsupportedVersion(3))
    ));
    // Version 2 without inferred paths is accepted and equals version 1.
    let mut doc = valid();
    doc["version"] = json!(2);
    assert_eq!(decode(&doc).unwrap(), decode(&valid()).unwrap());
    assert!(matches!(
        Registry::from_json(b"not json"),
        Err(ProjectError::Schema(_))
    ));
    assert!(matches!(
        Registry::from_json(b""),
        Err(ProjectError::Schema(_))
    ));
}

#[test]
fn dangling_and_duplicate_references_are_rejected() {
    let cases: Vec<(&str, Edit, Check)> = vec![
        (
            "dangling binding",
            |v| v["bindings"][0]["project"] = json!(ID_A.replace('1', "3")),
            |e| matches!(e, ProjectError::UnknownProject(_)),
        ),
        (
            "dangling assignment",
            |v| v["assignments"][0]["project"] = json!(ID_A.replace('1', "3")),
            |e| matches!(e, ProjectError::UnknownProject(_)),
        ),
        (
            "duplicate id",
            |v| v["projects"][1]["id"] = json!(ID_A),
            |e| matches!(e, ProjectError::DuplicateEntry),
        ),
        (
            "duplicate name",
            |v| v["projects"][1]["name"] = json!("APP"),
            |e| matches!(e, ProjectError::DuplicateName(_)),
        ),
        (
            "duplicate repo",
            |v| v["projects"][1]["repo"] = json!("github.com/o/app"),
            |e| matches!(e, ProjectError::DuplicateRepo { .. }),
        ),
        (
            "duplicate binding",
            |v| {
                let b = v["bindings"][0].clone();
                v["bindings"].as_array_mut().unwrap().push(b)
            },
            |e| matches!(e, ProjectError::DuplicateEntry),
        ),
        (
            "conflicting binding",
            |v| {
                let mut b = v["bindings"][0].clone();
                b["project"] = json!(ID_A);
                v["bindings"].as_array_mut().unwrap().push(b)
            },
            |e| matches!(e, ProjectError::DuplicateEntry),
        ),
        (
            "duplicate assignment",
            |v| {
                let mut a = v["assignments"][0].clone();
                a["project"] = json!(ID_B);
                v["assignments"].as_array_mut().unwrap().push(a)
            },
            |e| matches!(e, ProjectError::DuplicateEntry),
        ),
    ];
    for (label, mutate, expected) in cases {
        let mut doc = valid();
        mutate(&mut doc);
        let error = decode(&doc).unwrap_err();
        assert!(expected(&error), "{label}: {error:?}");
    }
}

#[test]
fn values_a_mutation_would_refuse_are_rejected() {
    let cases: Vec<(&str, Edit)> = vec![
        ("empty name", |v| v["projects"][0]["name"] = json!("")),
        ("padded name", |v| v["projects"][0]["name"] = json!(" app")),
        ("control in name", |v| {
            v["projects"][0]["name"] = json!("a\nb")
        }),
        ("long name", |v| {
            v["projects"][0]["name"] = json!("x".repeat(65))
        }),
        ("uppercase id", |v| {
            v["projects"][0]["id"] = json!(ID_A.to_uppercase().replace('1', "A"))
        }),
        ("non-id", |v| v["projects"][0]["id"] = json!("app")),
        ("unnormalized repo", |v| {
            v["projects"][0]["repo"] = json!("GitHub.com/o/app")
        }),
        ("relative path", |v| v["bindings"][0]["path"] = json!("t")),
        ("trailing slash path", |v| {
            v["bindings"][0]["path"] = json!("/t/")
        }),
        ("empty device", |v| v["bindings"][0]["device"] = json!("")),
        ("long device", |v| {
            v["assignments"][0]["device"] = json!("d".repeat(129))
        }),
        ("bad session", |v| {
            v["assignments"][0]["session"] = json!("../x")
        }),
        ("long session", |v| {
            v["assignments"][0]["session"] = json!("s".repeat(65))
        }),
    ];
    for (label, mutate) in cases {
        let mut doc = valid();
        mutate(&mut doc);
        assert!(decode(&doc).is_err(), "{label}");
    }
}

#[test]
fn counts_and_input_size_are_bounded() {
    let mut doc = valid();
    doc["assignments"] = json!(
        (0..MAX_ASSIGNMENTS + 1)
            .map(|i| json!({"device": "local", "session": format!("s{i}"), "project": ID_A}))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        decode(&doc),
        Err(ProjectError::LimitReached(Limit::Assignments))
    ));
    let mut doc = valid();
    doc["bindings"] = json!(
        (0..MAX_BINDINGS + 1)
            .map(|i| json!({"device": "local", "path": format!("/p{i}"), "project": ID_A}))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        decode(&doc),
        Err(ProjectError::LimitReached(Limit::Bindings))
    ));
    let mut doc = valid();
    doc["projects"] = json!(
        (0..MAX_PROJECTS + 1)
            .map(|i| json!({"id": uuid::Uuid::new_v4().to_string(), "name": format!("p{i}")}))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        decode(&doc),
        Err(ProjectError::LimitReached(Limit::Projects))
    ));
    let oversize = vec![b' '; Registry::MAX_JSON_BYTES + 1];
    assert!(matches!(
        Registry::from_json(&oversize),
        Err(ProjectError::TooLarge)
    ));
}

#[test]
fn decode_errors_never_echo_document_content() {
    let mut doc = valid();
    doc["projects"][0]["super-secret-field"] = json!(1);
    let text = decode(&doc).unwrap_err().to_string();
    assert!(!text.contains("super-secret-field"), "{text}");
}

#[test]
fn writing_refuses_what_reading_would_refuse() {
    let mut registry = Registry::new();
    let project = registry.create_project("p", None).unwrap();
    let dev = device(&format!("ssh:{}", "d".repeat(100)));
    for index in 0..MAX_BINDINGS {
        let path = format!("/{}/{index}", "x".repeat(900));
        registry.bind(dev.clone(), &path, &project.id).unwrap();
    }
    assert!(matches!(registry.to_json(), Err(ProjectError::TooLarge)));
    // A realistically full registry still round-trips under the cap.
    let mut registry = Registry::new();
    let project = registry.create_project("p", None).unwrap();
    for index in 0..MAX_BINDINGS {
        registry
            .bind(device("local"), &format!("/work/{index}"), &project.id)
            .unwrap();
    }
    for index in 0..MAX_ASSIGNMENTS {
        registry
            .assign(session("local", &format!("s{index}")), &project.id)
            .unwrap();
    }
    let bytes = registry.to_json().unwrap();
    assert!(bytes.len() <= Registry::MAX_JSON_BYTES);
    assert_eq!(Registry::from_json(&bytes).unwrap(), registry);
}

#[test]
fn check_saveable_agrees_with_to_json_at_the_size_cap() {
    let mut registry = Registry::new();
    let project = registry.create_project("p", None).unwrap();
    let dev = device(&format!("ssh:{}", "d".repeat(100)));
    let mut index = 0;
    while registry.check_saveable().is_ok() {
        // Both sides agree before the cap...
        assert!(registry.to_json().is_ok());
        for _ in 0..200 {
            let path = format!("/{}/{index}", "x".repeat(900));
            index += 1;
            registry.bind(dev.clone(), &path, &project.id).unwrap();
        }
        assert!(
            index < MAX_BINDINGS,
            "the cap must be hit before the count limit"
        );
    }
    // ...and after it.
    assert!(matches!(
        registry.check_saveable(),
        Err(ProjectError::TooLarge)
    ));
    assert!(matches!(registry.to_json(), Err(ProjectError::TooLarge)));
}

#[test]
fn the_borrowing_encoder_writes_the_documented_schema() {
    let mut registry = Registry::new();
    let project = registry
        .create_project("p", Some(key("git@github.com:a/b.git")))
        .unwrap();
    registry
        .bind(device("local"), "/work", &project.id)
        .unwrap();
    registry
        .assign(session("local", "s1"), &project.id)
        .unwrap();
    let value: Value = serde_json::from_slice(&registry.to_json().unwrap()).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["projects"][0]["repo"], "github.com/a/b");
    assert_eq!(value["bindings"][0]["path"], "/work");
    assert_eq!(value["assignments"][0]["session"], "s1");
    assert_eq!(
        Registry::from_json(&registry.to_json().unwrap()).unwrap(),
        registry
    );
}

fn valid_with_inferred() -> Value {
    json!({
        "version": 2,
        "projects": [
            {"id": ID_A, "name": "app", "repo": "github.com/o/app"},
            {"id": ID_B, "name": "task"},
        ],
        "bindings": [{"device": "local", "path": "/t", "project": ID_B}],
        "inferred_paths": [
            {"device": "ssh:box", "path": "/srv/abc", "project": ID_B},
        ],
        "assignments": [{"device": "local", "session": "main", "project": ID_A}],
    })
}

#[test]
fn version_two_inferred_paths_round_trip_as_v2_only_when_present() {
    let registry = decode(&valid_with_inferred()).unwrap();
    let bytes = registry.to_json().unwrap();
    assert_eq!(Registry::from_json(&bytes).unwrap(), registry);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["version"], 2);
    assert_eq!(value["inferred_paths"][0]["path"], "/srv/abc");
    // The same registry without its inference writes version 1 with no field.
    let mut without = registry.clone();
    without.inferred.clear();
    let v1: Value = serde_json::from_slice(&without.to_json().unwrap()).unwrap();
    assert_eq!(v1["version"], 1);
    assert!(v1["inferred_paths"].is_null());
}

#[test]
fn unknown_inferred_fields_are_rejected() {
    let mut doc = valid_with_inferred();
    doc["inferred_paths"][0]["note"] = json!("extra");
    assert!(matches!(decode(&doc), Err(ProjectError::Schema(_))));
    let mut doc = valid_with_inferred();
    doc["inferred_paths_extra"] = json!(1);
    assert!(matches!(decode(&doc), Err(ProjectError::Schema(_))));
}

#[test]
fn dangling_duplicate_and_invalid_inferred_paths_are_rejected() {
    let cases: Vec<(&str, Edit, Check)> = vec![
        (
            "dangling inferred",
            |v| v["inferred_paths"][0]["project"] = json!(ID_A.replace('1', "3")),
            |e| matches!(e, ProjectError::UnknownProject(_)),
        ),
        (
            "duplicate inferred",
            |v| {
                let i = v["inferred_paths"][0].clone();
                v["inferred_paths"].as_array_mut().unwrap().push(i)
            },
            |e| matches!(e, ProjectError::DuplicateEntry),
        ),
        (
            "relative inferred path",
            |v| v["inferred_paths"][0]["path"] = json!("abc"),
            |e| matches!(e, ProjectError::InvalidPath),
        ),
        (
            "trailing-slash inferred path",
            |v| v["inferred_paths"][0]["path"] = json!("/srv/abc/"),
            |e| matches!(e, ProjectError::InvalidPath),
        ),
    ];
    for (label, mutate, expected) in cases {
        let mut doc = valid_with_inferred();
        mutate(&mut doc);
        let error = decode(&doc).unwrap_err();
        assert!(expected(&error), "{label}: {error:?}");
    }
}

#[test]
fn manual_plus_inferred_paths_share_the_binding_budget() {
    let mut doc = valid_with_inferred();
    doc["bindings"] = json!(
        (0..MAX_BINDINGS)
            .map(|i| json!({"device": "local", "path": format!("/p{i}"), "project": ID_B}))
            .collect::<Vec<_>>()
    );
    doc["inferred_paths"] = json!(
        (0..MAX_BINDINGS)
            .map(|i| json!({"device": "local", "path": format!("/q{i}"), "project": ID_B}))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        decode(&doc),
        Err(ProjectError::LimitReached(Limit::Bindings))
    ));
}

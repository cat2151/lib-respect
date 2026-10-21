use std::fs;

use lib_respect::browser::{Screen, StarFilter, Store, UiPreferences};

#[test]
fn legacy_config_keeps_identity_and_modes_round_trip_with_safe_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::at(directory.path().into());
    let path = directory.path().join("config.json");
    fs::write(
        &path,
        r#"{"schema_version":1,"account":{"id":42,"login":"example"},"selected_repo":123}"#,
    )
    .unwrap();
    let mut config = store.config().unwrap();
    assert_eq!(config.ui, UiPreferences::default());
    for screen in [Screen::Projects, Screen::Repositories] {
        for stars_descending in [false, true] {
            for star_filter in [
                StarFilter::NotStarred,
                StarFilter::Starred,
                StarFilter::Both,
            ] {
                for include_own in [false, true] {
                    config.ui = UiPreferences {
                        screen,
                        stars_descending,
                        star_filter,
                        include_own,
                    };
                    store.save_config(&config).unwrap();
                    let reopened = Store::at(directory.path().into()).config().unwrap();
                    assert_eq!(reopened.ui, config.ui);
                    assert_eq!(reopened.account.as_ref().unwrap().id, 42);
                    assert_eq!(reopened.selected_repo, Some(123));
                }
            }
        }
    }
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["ui"]["screen"] = serde_json::json!("unknown_screen");
    let unknown = serde_json::to_string(&value).unwrap();
    fs::write(&path, &unknown).unwrap();
    assert!(store.config().is_err());
    assert!(store.save_config(&config).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), unknown);
    value["schema_version"] = serde_json::json!(999);
    let unsupported = serde_json::to_string(&value).unwrap();
    fs::write(&path, &unsupported).unwrap();
    assert!(store.save_config(&config).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), unsupported);
}

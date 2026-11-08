use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempConfig(PathBuf);

impl TempConfig {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(env::temp_dir().join(format!("ask-config-{}-{nonce}", std::process::id())))
    }

    fn path(&self) -> PathBuf {
        self.0.join(".ask/config")
    }

    fn load(&self) -> Config {
        Config::load_with(|| Some(self.path()))
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unavailable_config_uses_defaults_including_invalid_utf8_and_directory() {
    assert_eq!(Config::load_with(|| None), Config::default());
    let temporary = TempConfig::new();
    assert_eq!(temporary.load(), Config::default());
    fs::create_dir_all(temporary.path()).unwrap();
    assert_eq!(temporary.load(), Config::default());
    fs::remove_dir(temporary.path()).unwrap();
    fs::write(temporary.path(), b"theme=light\n\xff").unwrap();
    assert_eq!(temporary.load(), Config::default());
}

#[test]
fn parsing_preserves_valid_duplicates_and_ignores_unknown_or_invalid_values() {
    let temporary = TempConfig::new();
    fs::create_dir_all(temporary.path().parent().unwrap()).unwrap();
    fs::write(
        temporary.path(),
        concat!(
            "theme=LIGHT\nmodel= vendor/old \nauto=yes\n",
            "theme=bad\nmodel= \nauto=sometimes\n",
            " theme=dark\n model=ignored\n auto=off\nunknown=ignored\n",
            "model= vendor/current \nauto=0\nauto=TRUE\n",
        ),
    )
    .unwrap();
    assert_eq!(
        temporary.load(),
        Config {
            theme: ThemeMode::Light,
            model: Some("vendor/current".to_string()),
            auto: true,
        }
    );
}

#[test]
fn save_creates_directories_and_replaces_config_in_the_existing_format() {
    let temporary = TempConfig::new();
    let config = Config {
        theme: ThemeMode::Light,
        model: Some("vendor/model".to_string()),
        auto: true,
    };
    config.save_with(|| Some(temporary.path())).unwrap();
    assert_eq!(
        fs::read_to_string(temporary.path()).unwrap(),
        "theme=light\nmodel=vendor/model\nauto=on\n"
    );
    assert_eq!(temporary.load(), config);
    Config::default()
        .save_with(|| Some(temporary.path()))
        .unwrap();
    assert_eq!(
        fs::read_to_string(temporary.path()).unwrap(),
        "theme=dark\nauto=off\n"
    );
}

#[test]
fn failed_saves_propagate_without_removing_existing_paths() {
    let temporary = TempConfig::new();
    let config = Config::default();
    assert_eq!(
        config.save_with(|| None).unwrap_err().to_string(),
        "HOME is not set; cannot save ~/.ask/config."
    );
    fs::create_dir_all(temporary.path()).unwrap();
    fs::write(temporary.path().join("sentinel"), "keep").unwrap();
    assert!(config.save_with(|| Some(temporary.path())).is_err());
    assert_eq!(
        fs::read_to_string(temporary.path().join("sentinel")).unwrap(),
        "keep"
    );
    fs::remove_dir_all(temporary.path().parent().unwrap()).unwrap();
    fs::write(temporary.path().parent().unwrap(), "blocked directory").unwrap();
    assert!(config.save_with(|| Some(temporary.path())).is_err());
    assert_eq!(
        fs::read_to_string(temporary.path().parent().unwrap()).unwrap(),
        "blocked directory"
    );
}

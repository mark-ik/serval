//! Pelt's application-owned appearance setting.
//!
//! This is deliberately a small host seam.  The store is injected so callers
//! can choose durable storage (or an in-memory store in tests) without making
//! the settings projection depend on a particular application or document
//! engine.

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

use mere_surface_api::settings::{
    SettingControl, SettingMovement, SettingMutability, SettingOption, SettingScope,
    SettingSecurity, SettingSpec, SettingValue, SettingsError, SettingsProvider,
};
use workbench::SettingsRef;

pub const APPEARANCE_REFERENCE: &str = "pelt/appearance";
pub const CHROME_THEME_SETTING: &str = "chrome.theme";

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppearanceTheme {
    Dark,
    Light,
}

impl AppearanceTheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub(crate) const fn class(self) -> &'static str {
        match self {
            Self::Dark => "pelt-theme-dark",
            Self::Light => "pelt-theme-light",
        }
    }

    pub(crate) const fn action(self) -> &'static str {
        match self {
            Self::Dark => "appearance-dark",
            Self::Light => "appearance-light",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

pub trait AppearanceStore {
    fn theme(&self) -> AppearanceTheme;
    fn set_theme(&mut self, theme: AppearanceTheme) -> io::Result<()>;

    /// Whether this store survives a Pelt process restart.
    fn is_persistent(&self) -> bool {
        false
    }
}

impl<T: AppearanceStore + ?Sized> AppearanceStore for Box<T> {
    fn theme(&self) -> AppearanceTheme {
        (**self).theme()
    }

    fn set_theme(&mut self, theme: AppearanceTheme) -> io::Result<()> {
        (**self).set_theme(theme)
    }

    fn is_persistent(&self) -> bool {
        (**self).is_persistent()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryAppearanceStore {
    theme: AppearanceTheme,
}

impl InMemoryAppearanceStore {
    pub fn new(theme: AppearanceTheme) -> Self {
        Self { theme }
    }
}

impl Default for InMemoryAppearanceStore {
    fn default() -> Self {
        Self::new(AppearanceTheme::Dark)
    }
}

impl AppearanceStore for InMemoryAppearanceStore {
    fn theme(&self) -> AppearanceTheme {
        self.theme
    }

    fn set_theme(&mut self, theme: AppearanceTheme) -> io::Result<()> {
        self.theme = theme;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileAppearanceStore {
    path: PathBuf,
    theme: AppearanceTheme,
}

impl FileAppearanceStore {
    /// Loads a theme from `path`. A missing or malformed value uses the safe
    /// dark default; other I/O failures stay visible to the caller.
    pub fn load(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let theme = match fs::read_to_string(&path) {
            Ok(contents) => AppearanceTheme::parse(&contents).unwrap_or(AppearanceTheme::Dark),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidData
                ) =>
            {
                AppearanceTheme::Dark
            },
            Err(error) => return Err(error),
        };
        Ok(Self { path, theme })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AppearanceStore for FileAppearanceStore {
    fn theme(&self) -> AppearanceTheme {
        self.theme
    }

    fn set_theme(&mut self, theme: AppearanceTheme) -> io::Result<()> {
        let temporary = temporary_path(&self.path);
        let write_result = (|| {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(format!("{}\n", theme.as_str()).as_bytes())?;
            file.sync_all()?;
            drop(file);
            replace_file(&temporary, &self.path)
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        self.theme = theme;
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!(
        "pelt-appearance-{}-{sequence}.tmp",
        std::process::id()
    ))
}

#[cfg(target_os = "windows")]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination);
    }
    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // `ReplaceFileW` keeps the old complete file intact until the replacement
    // has succeeded, unlike delete-then-rename on Windows.
    let replaced = unsafe {
        ReplaceFileW(
            destination.as_ptr(),
            temporary.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}

pub struct AppearanceSettingsProvider<S> {
    store: S,
}

impl<S> AppearanceSettingsProvider<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }
}

impl<S: AppearanceStore> SettingsProvider for AppearanceSettingsProvider<S> {
    fn describe(&self, reference: &SettingsRef) -> Result<Vec<SettingSpec>, SettingsError> {
        if reference.0 != APPEARANCE_REFERENCE {
            return Err(SettingsError::UnsupportedReference(reference.clone()));
        }
        Ok(vec![SettingSpec {
            id: CHROME_THEME_SETTING.into(),
            label: "Chrome theme".into(),
            scope: SettingScope::Application,
            movement: SettingMovement::LocalOnly,
            mutability: SettingMutability::Live,
            security: SettingSecurity::Ordinary,
            control: SettingControl::Choice {
                options: vec![
                    SettingOption {
                        value: "dark".into(),
                        label: "Dark".into(),
                    },
                    SettingOption {
                        value: "light".into(),
                        label: "Light".into(),
                    },
                ],
            },
            value: SettingValue::Text(self.store.theme().as_str().into()),
        }])
    }

    fn apply(
        &mut self,
        reference: &SettingsRef,
        setting_id: &str,
        value: SettingValue,
    ) -> Result<(), SettingsError> {
        if reference.0 != APPEARANCE_REFERENCE {
            return Err(SettingsError::UnsupportedReference(reference.clone()));
        }
        if setting_id != CHROME_THEME_SETTING {
            return Err(SettingsError::UnknownSetting(setting_id.into()));
        }
        let SettingValue::Text(value) = value else {
            return Err(SettingsError::InvalidValue {
                setting_id: setting_id.into(),
                message: "expected Text".into(),
            });
        };
        let Some(theme) = AppearanceTheme::parse(&value) else {
            return Err(SettingsError::InvalidValue {
                setting_id: setting_id.into(),
                message: "expected one of dark, light".into(),
            });
        };
        self.store
            .set_theme(theme)
            .map_err(|error| SettingsError::Storage(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> SettingsRef {
        SettingsRef(APPEARANCE_REFERENCE.into())
    }

    #[test]
    fn provider_describes_live_local_theme_choice() {
        let provider = AppearanceSettingsProvider::new(InMemoryAppearanceStore::default());
        let spec = &provider.describe(&reference()).unwrap()[0];
        assert_eq!(spec.id, CHROME_THEME_SETTING);
        assert_eq!(spec.scope, SettingScope::Application);
        assert_eq!(spec.movement, SettingMovement::LocalOnly);
        assert_eq!(spec.mutability, SettingMutability::Live);
        assert_eq!(spec.security, SettingSecurity::Ordinary);
        assert_eq!(spec.value, SettingValue::Text("dark".into()));
        assert_eq!(
            spec.control,
            SettingControl::Choice {
                options: vec![
                    SettingOption {
                        value: "dark".into(),
                        label: "Dark".into(),
                    },
                    SettingOption {
                        value: "light".into(),
                        label: "Light".into(),
                    },
                ],
            }
        );
    }

    #[test]
    fn provider_rejects_unknown_refs_keys_types_and_values() {
        let mut provider = AppearanceSettingsProvider::new(InMemoryAppearanceStore::default());
        assert!(matches!(
            provider.describe(&SettingsRef("other".into())),
            Err(SettingsError::UnsupportedReference(_))
        ));
        assert!(matches!(
            provider.apply(&reference(), "other", SettingValue::Text("dark".into())),
            Err(SettingsError::UnknownSetting(_))
        ));
        assert!(matches!(
            provider.apply(
                &SettingsRef("other".into()),
                CHROME_THEME_SETTING,
                SettingValue::Text("dark".into())
            ),
            Err(SettingsError::UnsupportedReference(_))
        ));
        assert!(matches!(
            provider.apply(
                &reference(),
                CHROME_THEME_SETTING,
                SettingValue::Boolean(true)
            ),
            Err(SettingsError::InvalidValue { .. })
        ));
        assert!(matches!(
            provider.apply(
                &reference(),
                CHROME_THEME_SETTING,
                SettingValue::Text("blue".into())
            ),
            Err(SettingsError::InvalidValue { .. })
        ));
    }

    #[test]
    fn in_memory_store_updates_live_value() {
        let mut provider = AppearanceSettingsProvider::new(InMemoryAppearanceStore::default());
        assert!(!provider.store().is_persistent());
        provider
            .apply(
                &reference(),
                CHROME_THEME_SETTING,
                SettingValue::Text("light".into()),
            )
            .unwrap();
        assert_eq!(provider.store().theme(), AppearanceTheme::Light);
    }

    #[test]
    fn file_store_defaults_missing_or_invalid_and_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "pelt-appearance-{}-{}.theme",
            std::process::id(),
            TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&path);
        let mut store = FileAppearanceStore::load(&path).unwrap();
        assert_eq!(store.theme(), AppearanceTheme::Dark);
        assert!(store.is_persistent());
        store.set_theme(AppearanceTheme::Light).unwrap();
        assert_eq!(
            FileAppearanceStore::load(&path).unwrap().theme(),
            AppearanceTheme::Light
        );
        store.set_theme(AppearanceTheme::Dark).unwrap();
        assert_eq!(
            FileAppearanceStore::load(&path).unwrap().theme(),
            AppearanceTheme::Dark
        );
        fs::write(&path, "blue").unwrap();
        assert_eq!(
            FileAppearanceStore::load(&path).unwrap().theme(),
            AppearanceTheme::Dark
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn file_store_surfaces_nonrecoverable_load_errors() {
        let path = std::env::temp_dir().join(format!(
            "pelt-appearance-directory-{}-{}",
            std::process::id(),
            TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        assert!(FileAppearanceStore::load(&path).is_err());
        fs::remove_dir(path).unwrap();
    }
}

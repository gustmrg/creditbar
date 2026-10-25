//! UI strings. One JSON table per language in `locales/`, shared by the tray (Rust) and the
//! panel (which fetches the resolved table with `get_i18n`). Missing keys fall back to English.

use std::collections::HashMap;
use std::sync::OnceLock;

use creditbar_core::holidays::HOLIDAY_NAME_KEYS;
use serde::Serialize;

const ENGLISH: &str = "en-US";

/// Language tag → table. Matching is by exact tag first, then by language.
const LOCALES: [(&str, &str); 2] = [
    (ENGLISH, include_str!("../locales/en-US.json")),
    ("pt-BR", include_str!("../locales/pt-BR.json")),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct I18n {
    /// BCP 47 tag of the chosen table, also used for dates and numbers in the panel.
    locale: String,
    strings: HashMap<String, String>,
    /// Apple's Chinese holiday names → `holiday.*` key suffixes.
    holiday_keys: HashMap<&'static str, &'static str>,
}

static CURRENT: OnceLock<I18n> = OnceLock::new();

/// The table for the system language. `CREDITBAR_LANG` overrides it (for testing).
pub fn get() -> &'static I18n {
    CURRENT.get_or_init(|| {
        let requested = std::env::var("CREDITBAR_LANG")
            .ok()
            .or_else(sys_locale::get_locale)
            .unwrap_or_else(|| ENGLISH.to_owned());
        I18n::new(resolve(&requested))
    })
}

pub fn t(key: &str) -> String {
    get().text(key)
}

/// `t` with `{name}` placeholders replaced.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(t(key), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), value)
    })
}

/// Picks a table for a system locale such as `pt_BR.UTF-8` or `en_GB`; anything else is English.
fn resolve(requested: &str) -> &'static str {
    let tag = requested
        .split('.')
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    let lower = tag.to_ascii_lowercase();
    let language = lower.split('-').next().unwrap_or_default();
    LOCALES
        .iter()
        .map(|(tag, _)| *tag)
        .find(|tag| tag.to_ascii_lowercase() == lower)
        .or_else(|| {
            LOCALES.iter().map(|(tag, _)| *tag).find(|tag| {
                tag.split('-')
                    .next()
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(language)
            })
        })
        .unwrap_or(ENGLISH)
}

fn table(locale: &str) -> HashMap<String, String> {
    LOCALES
        .iter()
        .find(|(tag, _)| *tag == locale)
        .map(|(_, json)| serde_json::from_str(json).expect("valid locale table"))
        .unwrap_or_default()
}

impl I18n {
    fn new(locale: &str) -> Self {
        let mut strings = table(ENGLISH);
        strings.extend(table(locale));
        I18n {
            locale: locale.to_owned(),
            strings,
            holiday_keys: HOLIDAY_NAME_KEYS.into_iter().collect(),
        }
    }

    fn text(&self, key: &str) -> String {
        self.strings
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_system_locales() {
        assert_eq!(resolve("pt_BR.UTF-8"), "pt-BR");
        assert_eq!(resolve("pt-PT"), "pt-BR");
        assert_eq!(resolve("en-US"), "en-US");
        assert_eq!(resolve("en_GB.UTF-8"), "en-US");
        assert_eq!(resolve("zh_CN.UTF-8"), "en-US");
        assert_eq!(resolve("de-DE"), "en-US");
        assert_eq!(resolve("C"), "en-US");
    }

    #[test]
    fn every_table_has_the_english_keys() {
        let english = table(ENGLISH);
        for (tag, _) in LOCALES {
            let other = table(tag);
            let mut missing: Vec<_> = english.keys().filter(|k| !other.contains_key(*k)).collect();
            missing.sort();
            assert!(missing.is_empty(), "{tag} is missing {missing:?}");
            assert_eq!(other.len(), english.len(), "{tag} has keys English doesn't");
        }
    }

    #[test]
    fn every_holiday_has_a_string() {
        let english = table(ENGLISH);
        for (_, key) in HOLIDAY_NAME_KEYS {
            assert!(english.contains_key(&format!("holiday.{key}")), "{key}");
        }
    }
}

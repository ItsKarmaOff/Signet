//! Data model shared by every crate. Ports `packages/shared/src/types/*.ts` and `app/desktop/src/shared/types/*.ts`.
//!
//! Every type reads and writes the exact JSON of the Electron app (`state.json`...): camelCase fields, optional
//! fields skipped when absent, `#[serde(default)]` so old or partial files load.

use serde::{Deserialize, Deserializer, Serializer};

pub mod autofill;
pub mod data;
pub mod devtools;
pub mod extensions;
pub mod library;
pub mod link;
pub mod mods;
pub mod overlay;
pub mod passwords;
pub mod permissions;
pub mod settings;
pub mod spaces;
pub mod state;
pub mod tabs;
pub mod windows;

pub use autofill::*;
pub use data::*;
pub use devtools::*;
pub use extensions::*;
pub use library::*;
pub use link::*;
pub use mods::*;
pub use overlay::*;
pub use passwords::*;
pub use permissions::*;
pub use settings::*;
pub use spaces::*;
pub use state::*;
pub use tabs::*;
pub use windows::*;

pub use crate::i18n::{Locale, UiLanguage};

/// Serde helper: writes a whole `f64` as an integer (what JavaScript would have written).
pub(crate) fn f64_js<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(*v)
    }
}

/// Serde helper for `Option<f64>` (use with `skip_serializing_if = "Option::is_none"`).
pub(crate) fn opt_f64_js<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(v) => f64_js(v, s),
        None => s.serialize_none(),
    }
}

/// Serde helper for `Vec<f64>`.
pub(crate) fn vec_f64_js<S: Serializer>(v: &[f64], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    struct W<'a>(&'a f64);
    impl serde::Serialize for W<'_> {
        fn serialize<S2: Serializer>(&self, s: S2) -> Result<S2::Ok, S2::Error> {
            f64_js(self.0, s)
        }
    }
    let mut seq = s.serialize_seq(Some(v.len()))?;
    for x in v {
        seq.serialize_element(&W(x))?;
    }
    seq.end()
}

/// Serde helper: a missing, `null` or wrongly typed value becomes the default (never fails the whole file).
#[allow(dead_code)]
pub(crate) fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + for<'a> Deserialize<'a>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}


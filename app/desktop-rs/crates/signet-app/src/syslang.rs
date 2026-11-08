//! Preferred languages of the system, best first (BCP 47 or POSIX tags).

/// macOS: `NSLocale.preferredLanguages` (a GUI app started from Finder has no `LANG`). Elsewhere, and when the
/// system gives nothing, the usual environment variables.
pub fn system_languages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let langs: Vec<String> = objc2_foundation::NSLocale::preferredLanguages().iter().map(|s| s.to_string()).collect();
        if !langs.is_empty() {
            return langs;
        }
    }
    signet_core::i18n::system_languages_from_env()
}

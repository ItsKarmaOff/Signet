//! Profiles: one CEF `RequestContext` per profile (cookies, storage, cache, permissions).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cef::*;

use crate::runtime::{data_dir, profile_cache_path, with_state};
use crate::types::{ClearData, ProfileKind};

wrap_request_context_handler! {
    struct ProfileReady { ready: Arc<AtomicBool> }

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, _ctx: Option<&mut RequestContext>) {
            self.ready.store(true, Ordering::SeqCst);
        }
    }
}

/// Get or create the request context of a profile. Persistent profiles live under
/// `<data>/cef/profiles/<id>`; private ones are in memory (empty cache path).
pub(crate) fn context_for(profile: &ProfileKind) -> Option<RequestContext> {
    let key = profile.key();
    if let Some(Some(c)) = with_state(|st| st.contexts.get(&key).cloned()) {
        return Some(c);
    }
    let cache = match profile {
        ProfileKind::Persistent(_) => {
            let p = profile_cache_path(&data_dir()?, &key)?;
            p.to_string_lossy().to_string()
        }
        ProfileKind::Private(_) => String::new(),
    };
    let settings = RequestContextSettings {
        size: std::mem::size_of::<RequestContextSettings>(),
        cache_path: CefString::from(cache.as_str()),
        persist_session_cookies: matches!(profile, ProfileKind::Persistent(_)) as i32,
        ..Default::default()
    };
    let ready = Arc::new(AtomicBool::new(false));
    let mut handler = ProfileReady::new(ready.clone());
    let started = Instant::now();
    let ctx = request_context_create_context(Some(&settings), Some(&mut handler))?;
    // Chrome-style contexts initialize asynchronously; browsers need an initialized context.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut spins = 0u32;
    while !ready.load(Ordering::SeqCst) && Instant::now() < deadline {
        do_message_loop_work();
        std::thread::sleep(Duration::from_millis(2));
        spins += 1;
    }
    crate::startup::mark_with(|| format!("web.request_context {key} ready in {:.1} ms ({spins} pump+sleep(2 ms) rounds, main thread blocked)", started.elapsed().as_secs_f64() * 1000.0));
    if !ready.load(Ordering::SeqCst) {
        eprintln!("[signet-web] request context for {key} was not initialized in time");
    }
    crate::scheme::register_factories(&ctx);
    crate::extension::apply_preferences(&ctx);
    with_state(|st| st.contexts.insert(key, ctx.clone()));
    Some(ctx)
}

/// Forget a private profile's context (drops its in-memory data once its views are closed).
pub fn drop_profile(profile: &ProfileKind) {
    let key = profile.key();
    with_state(|st| st.contexts.remove(&key));
}

/// Clear cookies, cache and storage of a profile.
pub fn clear_profile_cache(profile: &ProfileKind) {
    if let Some(ctx) = context_for(profile) {
        ctx.clear_http_cache(None);
        ctx.close_all_connections(None);
    }
}

struct Countdown {
    left: usize,
    done: Option<Box<dyn FnOnce()>>,
}

type Shared = Arc<std::sync::Mutex<Countdown>>;

fn tick(c: &Shared) {
    let done = c.lock().ok().and_then(|mut g| {
        g.left = g.left.saturating_sub(1);
        if g.left == 0 { g.done.take() } else { None }
    });
    if let Some(d) = done {
        d();
    }
}

wrap_completion_callback! {
    struct Complete { counter: Shared }

    impl CompletionCallback {
        fn on_complete(&self) {
            tick(&self.counter);
        }
    }
}

wrap_delete_cookies_callback! {
    struct CookiesDeleted { counter: Shared }

    impl DeleteCookiesCallback {
        fn on_complete(&self, _num_deleted: ::std::os::raw::c_int) {
            tick(&self.counter);
        }
    }
}

/// Clear browsing data of a profile (the "clear history" action and private sessions): cookies,
/// HTTP cache, saved HTTP credentials, certificate exceptions, open connections. `done` runs on the
/// main thread when everything finished. Web storage (localStorage, IndexedDB...) has no profile-wide
/// call in CEF: use `WebView::clear_site_data` per origin, or close the views and `drop_profile`
/// for a private session.
pub fn clear_profile_data(profile: &ProfileKind, what: ClearData, done: impl FnOnce() + 'static) {
    let Some(ctx) = context_for(profile) else { return done() };
    let steps = what.cookies as usize + what.cache as usize + what.http_auth as usize + what.certificate_exceptions as usize + what.connections as usize;
    if steps == 0 {
        return done();
    }
    let counter: Shared = Arc::new(std::sync::Mutex::new(Countdown { left: steps, done: Some(Box::new(done)) }));
    if what.cookies {
        match ctx.cookie_manager(None) {
            Some(cm) => {
                let mut cb = CookiesDeleted::new(counter.clone());
                if cm.delete_cookies(None, None, Some(&mut cb)) != 1 {
                    tick(&counter);
                }
            }
            None => tick(&counter),
        }
    }
    if what.cache {
        ctx.clear_http_cache(Some(&mut Complete::new(counter.clone())));
    }
    if what.http_auth {
        ctx.clear_http_auth_credentials(Some(&mut Complete::new(counter.clone())));
    }
    if what.certificate_exceptions {
        ctx.clear_certificate_exceptions(Some(&mut Complete::new(counter.clone())));
    }
    if what.connections {
        ctx.close_all_connections(Some(&mut Complete::new(counter.clone())));
    }
}

/// Delete cookies of a profile: those matching `url` (and `name`) or all when both are `None`.
/// Returns false when the cookie manager is unavailable.
pub fn delete_cookies(profile: &ProfileKind, url: Option<&str>, name: Option<&str>) -> bool {
    let Some(cm) = context_for(profile).and_then(|c| c.cookie_manager(None)) else { return false };
    let (u, n) = (url.map(CefString::from), name.map(CefString::from));
    cm.delete_cookies(u.as_ref(), n.as_ref(), None) == 1
}

/// Spell checking of a profile: on/off and the dictionaries (BCP 47 codes such as `en-US`, `fr`).
/// Returns an error text when Chromium refuses a preference. Applies to all views of the profile.
pub fn set_spellcheck(profile: &ProfileKind, enabled: bool, languages: &[&str]) -> Result<(), String> {
    let ctx = context_for(profile).ok_or("no request context")?;
    let mut err = CefString::default();
    // The dictionaries decide on every platform; the enable flag is best effort (not settable on all builds).
    let mut v = value_create().ok_or("value")?;
    v.set_bool(enabled as i32);
    let _ = ctx.set_preference(Some(&CefString::from("browser.enable_spellchecking")), Some(&mut v), Some(&mut err));
    let mut list = list_value_create().ok_or("list")?;
    let langs: &[&str] = if enabled { languages } else { &[] };
    list.set_size(langs.len());
    for (i, l) in langs.iter().enumerate() {
        list.set_string(i, Some(&CefString::from(*l)));
    }
    let mut lv = value_create().ok_or("value")?;
    lv.set_list(Some(&mut list));
    if ctx.set_preference(Some(&CefString::from("spellcheck.dictionaries")), Some(&mut lv), Some(&mut err)) != 1 {
        return Err(format!("spellcheck.dictionaries: {err}"));
    }
    Ok(())
}

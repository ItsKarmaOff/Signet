//! Entry point of the CEF subprocess executable (`signet-helper`).

use cef::{sys, *};

use crate::app::SignetApp;

/// Run as a CEF subprocess (renderer, GPU, utility...). Returns the process exit code.
/// On macOS the helper lives in `Signet.app/Contents/Frameworks/Signet Helper*.app/Contents/MacOS/`.
pub fn run_subprocess() -> i32 {
    crate::startup::mark("subprocess.main");
    #[cfg(target_os = "macos")]
    let _loader = {
        let exe = std::env::current_exe().unwrap_or_default();
        let loader = cef::library_loader::LibraryLoader::new(&exe, true);
        if !loader.load() {
            eprintln!("signet-helper: cannot load the CEF framework");
            return 1;
        }
        loader
    };
    crate::startup::mark("subprocess.cef_loaded");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let ptype = cef::command_line_get_global().map(|c| cef::CefString::from(&cef::ImplCommandLine::switch_value(&c, Some(&cef::CefString::from("type")))).to_string()).unwrap_or_default();
    crate::app::rlog(&format!("subprocess start type={ptype} bundle_switch={:?}", std::env::var("SIGNET_WEB_BUNDLE").ok()));
    let mut app = SignetApp::build(true);
    execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut())
}

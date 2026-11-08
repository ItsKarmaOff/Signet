//! CEF subprocess executable (renderer, GPU, utility processes). All logic lives in signet-web.

fn main() {
    std::process::exit(signet_web::run_subprocess());
}

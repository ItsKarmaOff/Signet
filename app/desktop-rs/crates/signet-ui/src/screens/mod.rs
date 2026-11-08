//! Screens: one module per surface (port of `app/desktop/src/renderer/src/*`). See PORTING.md section 8.
//! Each module is owned by one agent at a time; do not edit another module.

pub mod devtools;
pub mod main_window;
pub mod overlay;
pub mod settings;
pub mod sidebar;
pub mod tabs;
pub mod toolbar;
pub mod windows;

/// Platform-specific input implementations.
///
/// Each platform submodule exposes a `PlatformInjector` type that implements
/// `macronova_core::platform::input::InputInjector`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::PlatformInjector;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::PlatformInjector;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::PlatformInjector;

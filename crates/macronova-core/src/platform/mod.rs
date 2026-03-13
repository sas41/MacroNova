/// Platform abstraction layer.
///
/// This module exposes traits for the platform-specific capabilities that
/// MacroNova needs, with implementations gated by `#[cfg(target_os)]`.
///
/// Adding a new platform means implementing these traits in a new submodule
/// and wiring them up in the `cfg` blocks at the bottom of each trait file.
pub mod input;

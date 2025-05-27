pub(crate) mod error;
pub mod microphone_listener;
pub mod screen_capture_kit;

pub use microphone_listener::*;
pub use screen_capture_kit::*;

#[cfg(test)]
mod tests {
  #[test]
  fn test_windows_module_loads() {
    // Simple test to ensure the Windows module compiles and loads correctly
    assert!(true);
  }
}

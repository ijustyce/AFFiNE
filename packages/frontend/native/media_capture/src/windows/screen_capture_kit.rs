use std::{
  collections::HashSet,
  ffi::OsString,
  os::windows::ffi::OsStringExt,
  sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, LazyLock, RwLock,
  },
  thread,
  time::Duration,
};

use napi::{
  bindgen_prelude::{Buffer, Error, Result, Status},
  threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;

// Windows API imports
use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE}; // HWND removed
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Diagnostics::ToolHelp::{
  CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{GetModuleFileNameExW, GetProcessImageFileNameW};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

// Import the function from microphone_listener
use crate::windows::microphone_listener::is_process_actively_using_microphone;

// Type alias to match macOS API
pub type AudioObjectID = u32;

// Global storage for running applications (Windows equivalent of macOS audio process list)
static RUNNING_APPLICATIONS: LazyLock<RwLock<Vec<u32>>> =
  LazyLock::new(|| RwLock::new(get_running_processes()));

// Simple counter for generating unique handles
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);

// Global storage for active watchers
static ACTIVE_APP_WATCHERS: LazyLock<
  RwLock<Vec<(u32, u32, Arc<ThreadsafeFunction<(), ()>>, Arc<AtomicBool>)>>,
> = LazyLock::new(|| RwLock::new(Vec::new()));

static ACTIVE_LIST_WATCHERS: LazyLock<
  RwLock<Vec<(u32, Arc<ThreadsafeFunction<(), ()>>, Arc<AtomicBool>)>>,
> = LazyLock::new(|| RwLock::new(Vec::new()));

#[napi]
pub struct Application {
  pub(crate) process_id: i32,
  pub(crate) name: String,
}

#[napi]
impl Application {
  #[napi(constructor)]
  pub fn new(process_id: i32) -> Self {
    let name =
      get_process_name(process_id as u32).unwrap_or_else(|| format!("Process {}", process_id));
    Self { process_id, name }
  }

  #[napi(getter)]
  pub fn process_id(&self) -> i32 {
    self.process_id
  }

  #[napi(getter)]
  pub fn process_group_id(&self) -> i32 {
    // Windows doesn't have process groups like Unix, return the process ID
    self.process_id
  }

  #[napi(getter)]
  pub fn bundle_identifier(&self) -> String {
    // For Windows, return the fully-qualified path to the .exe on disk
    get_process_executable_path(self.process_id as u32).unwrap_or_default()
  }

  #[napi(getter)]
  pub fn name(&self) -> String {
    self.name.clone()
  }

  #[napi(getter)]
  pub fn icon(&self) -> Buffer {
    // For now, return empty buffer. In a full implementation, you would extract
    // the icon from the executable file using Windows APIs
    Buffer::from(Vec::<u8>::new())
  }
}

#[napi]
pub struct TappableApplication {
  pub(crate) app: Application,
  pub(crate) object_id: u32, // Windows equivalent of AudioObjectID
}

#[napi]
impl TappableApplication {
  #[napi(constructor)]
  pub fn new(object_id: u32) -> Self {
    let app = Application::new(object_id as i32);
    Self { app, object_id }
  }

  #[napi(factory)]
  pub fn from_application(app: &Application, object_id: u32) -> Self {
    Self {
      app: Application {
        process_id: app.process_id,
        name: app.name.clone(),
      },
      object_id,
    }
  }

  #[napi(getter)]
  pub fn process_id(&self) -> i32 {
    self.app.process_id
  }

  #[napi(getter)]
  pub fn process_group_id(&self) -> i32 {
    self.app.process_group_id()
  }

  #[napi(getter)]
  pub fn bundle_identifier(&self) -> String {
    self.app.bundle_identifier()
  }

  #[napi(getter)]
  pub fn name(&self) -> String {
    self.app.name.clone()
  }

  #[napi(getter)]
  pub fn object_id(&self) -> u32 {
    self.object_id
  }

  #[napi(getter)]
  pub fn icon(&self) -> Buffer {
    self.app.icon()
  }

  #[napi(getter)]
  pub fn is_running(&self) -> bool {
    is_process_actively_using_microphone(self.app.process_id as u32).unwrap_or(false)
  }

  #[napi]
  pub fn tap_audio(
    &self,
    _audio_stream_callback: ThreadsafeFunction<napi::bindgen_prelude::Float32Array, ()>,
  ) -> Result<AudioCaptureSession> {
    // For Windows, we don't have the same audio tapping capabilities as macOS
    // This would need to be implemented using WASAPI or other Windows audio APIs
    Err(Error::new(
      Status::GenericFailure,
      "Audio tapping not yet implemented for Windows",
    ))
  }
}

#[napi]
pub struct ApplicationListChangedSubscriber {
  handle: u32,
  // We'll store the callback and manage it through a background thread
  _callback: Arc<ThreadsafeFunction<(), ()>>,
}

#[napi]
impl ApplicationListChangedSubscriber {
  #[napi]
  pub fn unsubscribe(&self) -> Result<()> {
    if let Ok(mut watchers) = ACTIVE_LIST_WATCHERS.write() {
      if let Some(pos) = watchers
        .iter()
        .position(|(handle, _, _)| *handle == self.handle)
      {
        let (_, _, should_stop) = &watchers[pos];
        should_stop.store(true, Ordering::Relaxed);
        watchers.remove(pos);
      }
    }
    Ok(())
  }
}

#[napi]
pub struct ApplicationStateChangedSubscriber {
  handle: u32,
  process_id: u32,
  _callback: Arc<ThreadsafeFunction<(), ()>>,
}

#[napi]
impl ApplicationStateChangedSubscriber {
  #[napi(getter)]
  pub fn process_id(&self) -> u32 {
    self.process_id
  }

  #[napi]
  pub fn unsubscribe(&self) {
    if let Ok(mut watchers) = ACTIVE_APP_WATCHERS.write() {
      if let Some(pos) = watchers
        .iter()
        .position(|(handle, _, _, _)| *handle == self.handle)
      {
        let (_, _, _, should_stop) = &watchers[pos];
        should_stop.store(true, Ordering::Relaxed);
        watchers.remove(pos);
      }
    }
  }
}

#[napi]
pub struct ShareableContent {
  // Windows doesn't need an inner SCShareableContent equivalent
}

#[napi]
#[derive(Default)]
pub struct RecordingPermissions {
  pub audio: bool,
  pub screen: bool,
}

#[napi]
impl ShareableContent {
  #[napi]
  pub fn on_application_list_changed(
    callback: ThreadsafeFunction<(), ()>,
  ) -> Result<ApplicationListChangedSubscriber> {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let callback_arc = Arc::new(callback);

    // Start monitoring for application list changes
    start_list_monitoring(handle, callback_arc.clone());

    Ok(ApplicationListChangedSubscriber {
      handle,
      _callback: callback_arc,
    })
  }

  #[napi]
  pub fn on_app_state_changed(
    app: &TappableApplication,
    callback: ThreadsafeFunction<(), ()>,
  ) -> Result<ApplicationStateChangedSubscriber> {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let process_id = app.process_id() as u32;
    let callback_arc = Arc::new(callback);

    // Start monitoring for this specific process's microphone state
    start_process_monitoring(handle, process_id, callback_arc.clone());

    Ok(ApplicationStateChangedSubscriber {
      handle,
      process_id,
      _callback: callback_arc,
    })
  }

  #[napi(constructor)]
  pub fn new() -> Self {
    unsafe {
      CoInitializeEx(None, COINIT_MULTITHREADED)
        .ok()
        .unwrap_or_else(|_| {
          // COM initialization failed, but we can't return an error from constructor
          // This is typically not fatal as COM might already be initialized
        });
    }
    Self {}
  }

  #[napi]
  pub fn applications(&self) -> Result<Vec<TappableApplication>> {
    let processes = RUNNING_APPLICATIONS.read().map_err(|_| {
      Error::new(
        Status::GenericFailure,
        "Failed to read running applications",
      )
    })?;

    let mut apps = Vec::new();
    for &process_id in processes.iter() {
      let app = TappableApplication::new(process_id);
      if !app.name().is_empty() && app.name() != format!("Process {}", process_id) {
        apps.push(app);
      }
    }
    Ok(apps)
  }

  #[napi]
  pub fn application_with_process_id(&self, process_id: u32) -> Option<Application> {
    if is_process_running(process_id) {
      Some(Application::new(process_id as i32))
    } else {
      None
    }
  }

  #[napi]
  pub fn tappable_application_with_process_id(
    &self,
    process_id: u32,
  ) -> Option<TappableApplication> {
    if is_process_running(process_id) {
      Some(TappableApplication::new(process_id))
    } else {
      None
    }
  }

  #[napi]
  pub fn tap_global_audio(
    _excluded_processes: Option<Vec<&TappableApplication>>,
    _audio_stream_callback: ThreadsafeFunction<napi::bindgen_prelude::Float32Array, ()>,
  ) -> Result<AudioCaptureSession> {
    // Windows global audio tapping would be implemented here
    Err(Error::new(
      Status::GenericFailure,
      "Global audio tapping not yet implemented for Windows",
    ))
  }
}

// Placeholder for Windows audio capture session
#[napi]
pub struct AudioCaptureSession {
  _handle: u32,
}

#[napi]
impl AudioCaptureSession {
  #[napi]
  pub fn stop(&self) {
    // Windows implementation would stop audio capture
  }

  #[napi(getter)]
  pub fn sample_rate(&self) -> f64 {
    48000.0 // Default sample rate
  }

  #[napi(getter)]
  pub fn channels(&self) -> u32 {
    2 // Stereo
  }

  #[napi(getter)]
  pub fn actual_sample_rate(&self) -> f64 {
    48000.0 // Default sample rate
  }
}

// Helper functions for Windows process management

fn get_running_processes() -> Vec<u32> {
  let mut processes_set = HashSet::new(); // Use HashSet to avoid duplicates from the start
  unsafe {
    let h_snapshot_result = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);

    let h_snapshot = match h_snapshot_result {
      Ok(handle) => {
        if handle == INVALID_HANDLE_VALUE {
          // eprintln!("CreateToolhelp32Snapshot returned INVALID_HANDLE_VALUE");
          return Vec::new();
        }
        handle
      }
      Err(_e) => {
        // eprintln!("CreateToolhelp32Snapshot failed: {:?}", e);
        return Vec::new();
      }
    };

    let mut pe32 = PROCESSENTRY32W::default();
    pe32.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    if Process32FirstW(h_snapshot, &mut pe32).is_ok() {
      loop {
        processes_set.insert(pe32.th32ProcessID);
        if Process32NextW(h_snapshot, &mut pe32).is_err() {
          break;
        }
      }
    }
    CloseHandle(h_snapshot).unwrap_or_else(|_e| {
      // eprintln!("CloseHandle failed for snapshot: {:?}", e);
    });
  }
  let mut processes_vec: Vec<u32> = processes_set.into_iter().collect();
  processes_vec.sort_unstable(); // Sort for consistent ordering, though not strictly necessary for functionality
  processes_vec
}

fn is_process_running(process_id: u32) -> bool {
  unsafe {
    match OpenProcess(PROCESS_QUERY_INFORMATION, false, process_id) {
      Ok(handle) => CloseHandle(handle).is_ok(),
      Err(_) => false,
    }
  }
}

fn get_process_name(pid: u32) -> Option<String> {
  unsafe {
    let process_handle =
      OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
    let mut buffer = [0u16; 260]; // MAX_PATH

    let length = GetModuleFileNameExW(Some(process_handle), None, &mut buffer);
    CloseHandle(process_handle).ok()?;

    if length == 0 {
      return None;
    }

    let os_string = OsString::from_wide(&buffer[0..length as usize]);
    let path_str = os_string.to_string_lossy().to_string();
    path_str.rsplit('\\').next().map(|s| s.to_string())
  }
}

fn get_process_executable_path(pid: u32) -> Option<String> {
  unsafe {
    let process_handle =
      OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
    let mut buffer = [0u16; 260]; // MAX_PATH

    let length = GetProcessImageFileNameW(process_handle, &mut buffer);
    CloseHandle(process_handle).ok()?;

    if length == 0 {
      return None;
    }

    let os_string = OsString::from_wide(&buffer[0..length as usize]);
    let path_str = os_string.to_string_lossy().to_string();
    Some(path_str)
  }
}

// Helper function to start monitoring a specific process
fn start_process_monitoring(
  handle: u32,
  process_id: u32,
  callback: Arc<ThreadsafeFunction<(), ()>>,
) {
  let should_stop = Arc::new(AtomicBool::new(false));
  let should_stop_clone = should_stop.clone();

  // Store the watcher info
  if let Ok(mut watchers) = ACTIVE_APP_WATCHERS.write() {
    watchers.push((handle, process_id, callback.clone(), should_stop.clone()));
  }

  // Start monitoring thread
  thread::spawn(move || {
    let mut last_state = false;

    loop {
      if should_stop_clone.load(Ordering::Relaxed) {
        break;
      }

      // Check current microphone state
      let current_state = is_process_actively_using_microphone(process_id).unwrap_or(false);

      // If state changed, trigger callback
      if current_state != last_state {
        let _ = callback.call(Ok(()), ThreadsafeFunctionCallMode::NonBlocking);
        last_state = current_state;
      }

      // Sleep for a short interval before checking again
      thread::sleep(Duration::from_millis(500));
    }
  });
}

// Helper function to start monitoring application list changes
fn start_list_monitoring(handle: u32, callback: Arc<ThreadsafeFunction<(), ()>>) {
  let should_stop = Arc::new(AtomicBool::new(false));
  let should_stop_clone = should_stop.clone();

  // Store the watcher info
  if let Ok(mut watchers) = ACTIVE_LIST_WATCHERS.write() {
    watchers.push((handle, callback.clone(), should_stop.clone()));
  }

  // Start monitoring thread
  thread::spawn(move || {
    let mut last_processes = get_running_processes();

    loop {
      if should_stop_clone.load(Ordering::Relaxed) {
        break;
      }

      // Check current process list
      let current_processes = get_running_processes();

      // If process list changed, trigger callback
      if current_processes != last_processes {
        let _ = callback.call(Ok(()), ThreadsafeFunctionCallMode::NonBlocking);
        last_processes = current_processes;

        // Update global process list
        if let Ok(mut apps) = RUNNING_APPLICATIONS.write() {
          *apps = last_processes.clone();
        }
      }

      // Sleep for a longer interval for process list changes
      thread::sleep(Duration::from_millis(2000));
    }
  });
}

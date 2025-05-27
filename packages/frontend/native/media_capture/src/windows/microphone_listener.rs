use std::{
  ffi::OsString,
  os::windows::ffi::OsStringExt,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
  },
};

use napi::{
  threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
  Result as NapiResult,
};
use napi_derive::napi;
use windows::{
  core::{Interface, Result},
  Win32::{
    Foundation::CloseHandle,
    Media::Audio::{
      eCapture, eCommunications, eConsole, AudioSessionState, AudioSessionStateActive,
      IAudioSessionControl, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionEvents,
      IAudioSessionEvents_Impl, IAudioSessionManager2, IAudioSessionNotification,
      IAudioSessionNotification_Impl, IMMDevice, IMMDeviceCollection, IMMDeviceEnumerator,
      MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    },
    System::{
      Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
      ProcessStatus::{GetModuleFileNameExW, GetProcessImageFileNameW},
      Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
  },
};
use windows_core::implement;

#[napi(object)]
pub struct AudioProcess {
  pub process_name: String,
  pub process_id: u32,
  pub device_id: String,
  pub device_name: String,
  pub is_running: bool,
}

#[napi(object)]
pub struct AudioDevice {
  pub device_id: String,
  pub device_name: String,
  pub is_default_communications: bool,
  pub is_default_console: bool,
  pub has_active_sessions: bool,
}

// Simple struct for callback data - not a NAPI object
#[derive(Clone)]
pub struct MicrophoneActivateCallback {
  pub is_running: bool,
  pub process_name: String,
  pub device_id: String,
  pub device_name: String,
}

#[implement(IAudioSessionEvents)]
struct SessionEvents {
  process_name: String,
  device_id: String,
  device_name: String,
  callback: Arc<ThreadsafeFunction<(bool, String, String, String)>>,
  ctrl: IAudioSessionControl,
  events_ref: Arc<Mutex<Option<IAudioSessionEvents>>>,
  is_running: Arc<AtomicBool>,
  mgr: IAudioSessionManager2,
}

impl IAudioSessionEvents_Impl for SessionEvents_Impl {
  fn OnChannelVolumeChanged(
    &self,
    _channelcount: u32,
    _newchannelvolumearray: *const f32,
    _changedchannel: u32,
    _eventcontext: *const windows_core::GUID,
  ) -> windows_core::Result<()> {
    Ok(())
  }

  fn OnDisplayNameChanged(
    &self,
    _newdisplayname: &windows_core::PCWSTR,
    _eventcontext: *const windows_core::GUID,
  ) -> windows_core::Result<()> {
    Ok(())
  }

  fn OnGroupingParamChanged(
    &self,
    _newgroupingparam: *const windows_core::GUID,
    _eventcontext: *const windows_core::GUID,
  ) -> windows_core::Result<()> {
    Ok(())
  }

  fn OnIconPathChanged(
    &self,
    _newiconpath: &windows_core::PCWSTR,
    _eventcontext: *const windows_core::GUID,
  ) -> windows_core::Result<()> {
    Ok(())
  }

  fn OnSessionDisconnected(
    &self,
    _disconnectreason: windows::Win32::Media::Audio::AudioSessionDisconnectReason,
  ) -> windows_core::Result<()> {
    if let Some(events) = self.events_ref.lock().unwrap().take() {
      unsafe { self.ctrl.UnregisterAudioSessionNotification(&events)? };
    }
    Ok(())
  }

  fn OnSimpleVolumeChanged(
    &self,
    _newvolume: f32,
    _newmute: windows_core::BOOL,
    _eventcontext: *const windows_core::GUID,
  ) -> windows_core::Result<()> {
    Ok(())
  }

  fn OnStateChanged(&self, newstate: AudioSessionState) -> windows_core::Result<()> {
    let is_recording = newstate == AudioSessionStateActive;

    let overall_is_running = if is_recording {
      // If this session is now active, set is_running to true
      self.is_running.store(true, Ordering::Relaxed);
      true
    } else {
      // If this session is now inactive, check if any other sessions are still active
      let (any_active, _) =
        get_mgr_audio_session_running_status(&self.mgr).unwrap_or((false, String::new()));
      self.is_running.store(any_active, Ordering::Relaxed);
      any_active
    };

    self.callback.call(
      Ok((
        overall_is_running,
        self.process_name.clone(),
        self.device_id.clone(),
        self.device_name.clone(),
      )),
      ThreadsafeFunctionCallMode::NonBlocking,
    );

    Ok(())
  }
}

#[implement(IAudioSessionNotification)]
struct SessionNotifier {
  _mgr: IAudioSessionManager2, // keep mgr alive
  device_id: String,
  device_name: String,
  ctrl: Mutex<Option<(IAudioSessionControl2, IAudioSessionEvents)>>, // keep the ctrl2 and events alive
  callback: Arc<ThreadsafeFunction<(bool, String, String, String)>>,
  is_running: Arc<AtomicBool>, // Add reference to the shared is_running state
}

impl SessionNotifier {
  fn new(
    mgr: &IAudioSessionManager2,
    device_id: String,
    device_name: String,
    callback: Arc<ThreadsafeFunction<(bool, String, String, String)>>,
    is_running: Arc<AtomicBool>,
  ) -> Self {
    Self {
      _mgr: mgr.clone(),
      device_id,
      device_name,
      ctrl: Default::default(),
      callback,
      is_running,
    }
  }

  fn refresh_state(&self, ctrl: &IAudioSessionControl) -> Result<()> {
    let ctrl2: IAudioSessionControl2 = ctrl.cast()?;
    let process_id = unsafe { ctrl2.GetProcessId()? };
    let process_name = match get_process_name(process_id) {
      Some(n) => n,
      None => unsafe { ctrl2.GetDisplayName()?.to_string()? },
    };
    // Skip system-sounds session
    // The `IsSystemSoundsSession` always true for unknown reason
    if process_name.contains("AudioSrv") {
      return Ok(());
    }

    // Active ⇒ microphone is recording
    if unsafe { ctrl.GetState()? } == AudioSessionStateActive {
      if let Ok(mut optional_ctrl) = self.ctrl.lock() {
        // Update the shared is_running state
        self.is_running.store(true, Ordering::Relaxed);

        let events_ref = Arc::new(Mutex::new(None));
        let events: IAudioSessionEvents = SessionEvents {
          callback: self.callback.clone(),
          process_name: process_name.clone(),
          device_id: self.device_id.clone(),
          device_name: self.device_name.clone(),
          events_ref: events_ref.clone(),
          ctrl: ctrl.clone(),
          is_running: self.is_running.clone(),
          mgr: self._mgr.clone(),
        }
        .into();
        let mut events_mut_ref = events_ref.lock().unwrap();
        *events_mut_ref = Some(events.clone());
        self.callback.call(
          Ok((
            true,
            process_name,
            self.device_id.clone(),
            self.device_name.clone(),
          )),
          ThreadsafeFunctionCallMode::NonBlocking,
        );
        unsafe { ctrl.RegisterAudioSessionNotification(&events)? };
        // keep the ctrl2 alive so that the notification can be called
        *optional_ctrl = Some((ctrl2, events));
      }
      return Ok(());
    }
    Ok(())
  }
}

impl IAudioSessionNotification_Impl for SessionNotifier_Impl {
  fn OnSessionCreated(
    &self,
    ctrl_ref: windows_core::Ref<'_, windows::Win32::Media::Audio::IAudioSessionControl>,
  ) -> windows_core::Result<()> {
    let Some(ctrl) = ctrl_ref.as_ref() else {
      return Ok(());
    };
    self.refresh_state(ctrl)?;
    Ok(())
  }
}

pub fn register_audio_device_status_callback(
  is_running: Arc<AtomicBool>,
  callback: Arc<ThreadsafeFunction<(bool, String, String, String)>>,
) -> Result<Vec<IAudioSessionNotification>> {
  unsafe {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    // Get all active capture devices
    let device_collection: IMMDeviceCollection =
      enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;

    let device_count = device_collection.GetCount()?;
    let mut session_notifiers = Vec::new();

    for i in 0..device_count {
      let device: IMMDevice = device_collection.Item(i)?;

      // Get device ID
      let device_id_pwstr = device.GetId()?;
      let device_id = device_id_pwstr.to_string()?;

      // Use device ID as device name for simplicity
      let device_name = format!("Audio Device {}", i);

      let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;

      let session_notifier = SessionNotifier::new(
        &mgr,
        device_id.clone(),
        device_name.clone(),
        callback.clone(),
        is_running.clone(),
      );

      let session_notifier_impl: IAudioSessionNotification = session_notifier.into();

      mgr.RegisterSessionNotification(&session_notifier_impl)?;

      // Check initial state for this device
      let (is_device_running, process_name) = get_mgr_audio_session_running_status(&mgr)?;
      if is_device_running {
        is_running.store(true, Ordering::Relaxed);
        callback.call(
          Ok((
            is_device_running,
            process_name,
            device_id.clone(),
            device_name.clone(),
          )),
          ThreadsafeFunctionCallMode::NonBlocking,
        );
      }

      session_notifiers.push(session_notifier_impl);
    }

    Ok(session_notifiers)
  }
}

#[napi]
pub struct MicrophoneListener {
  _session_notifiers: Vec<IAudioSessionNotification>, // keep the session_notifiers alive
  is_running: Arc<AtomicBool>,
}

#[napi]
impl MicrophoneListener {
  #[napi(constructor)]
  pub fn new(callback: ThreadsafeFunction<(bool, String, String, String)>) -> Self {
    unsafe {
      if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
        // If COM initialization fails, create a listener with empty notifiers
        return Self {
          is_running: Arc::new(AtomicBool::new(false)),
          _session_notifiers: Vec::new(),
        };
      }
    };

    let is_running = Arc::new(AtomicBool::new(false));

    let session_notifiers =
      match register_audio_device_status_callback(is_running.clone(), Arc::new(callback)) {
        Ok(notifiers) => notifiers,
        Err(_) => {
          // If registration fails, create a listener with empty notifiers
          Vec::new()
        }
      };

    Self {
      is_running,
      _session_notifiers: session_notifiers,
    }
  }

  pub fn is_running(&self) -> bool {
    self.is_running.load(Ordering::Relaxed)
  }

  // Static method to check if a specific process is using microphone
  // This is used by TappableApplication::is_running()
  pub fn is_process_using_microphone(process_id: u32) -> bool {
    // Use the proven get_all_audio_processes logic
    match get_all_audio_processes() {
      Ok(processes) => processes
        .iter()
        .any(|p| p.process_id == process_id && p.is_running),
      Err(_) => false,
    }
  }
}

fn get_mgr_audio_session_running_status(mgr: &IAudioSessionManager2) -> Result<(bool, String)> {
  let list: IAudioSessionEnumerator = unsafe { mgr.GetSessionEnumerator()? };
  let sessions = unsafe { list.GetCount()? };
  for idx in 0..sessions {
    let ctrl = unsafe { list.GetSession(idx)? };
    let ctrl2: IAudioSessionControl2 = ctrl.cast()?;
    let process_id = unsafe { ctrl2.GetProcessId()? };
    let process_name = match get_process_name(process_id) {
      Some(n) => n,
      None => unsafe { ctrl2.GetDisplayName()?.to_string()? },
    };
    // Skip system-sounds session
    // The `IsSystemSoundsSession` always true for unknown reason
    if process_name.contains("AudioSrv") {
      continue;
    }

    // Active ⇒ microphone is recording
    if unsafe { ctrl.GetState()? } == AudioSessionStateActive {
      return Ok((true, process_name));
    }
  }
  Ok((false, String::new()))
}

fn get_process_name(pid: u32) -> Option<String> {
  unsafe {
    // Open process with required access rights
    let process_handle =
      OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;

    // Create buffer for filename
    let mut buffer = [0u16; 260]; // MAX_PATH

    // Try GetModuleFileNameExW first (gives full path with extension)
    let length = GetModuleFileNameExW(
      Some(process_handle),
      None, // NULL for the process executable
      &mut buffer,
    );

    // If that fails, try GetProcessImageFileNameW
    let length = if length == 0 {
      GetProcessImageFileNameW(process_handle, &mut buffer)
    } else {
      length
    };

    // Clean up
    CloseHandle(process_handle).ok()?;

    if length == 0 {
      return None;
    }

    // Convert to OsString then to a regular String
    let os_string = OsString::from_wide(&buffer[0..length as usize]);

    // Extract the file name from the path
    let path_str = os_string.to_string_lossy().to_string();
    path_str.rsplit('\\').next().map(|s| s.to_string())
  }
}

#[napi]
pub fn list_audio_processes() -> NapiResult<Vec<AudioProcess>> {
  unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;
  };

  let result = get_all_audio_processes()
    .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;

  Ok(result)
}

#[napi]
pub fn list_audio_devices() -> NapiResult<Vec<AudioDevice>> {
  unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;
  };

  let result = get_all_audio_devices()
    .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;

  Ok(result)
}

fn get_all_audio_processes() -> Result<Vec<AudioProcess>> {
  unsafe {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let device_collection: IMMDeviceCollection =
      enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;

    let device_count = device_collection.GetCount()?;
    let mut all_processes = Vec::new();

    for i in 0..device_count {
      let device: IMMDevice = device_collection.Item(i)?;

      let device_id_pwstr = device.GetId()?;
      let device_id = device_id_pwstr.to_string()?;
      let device_name = format!("Audio Device {}", i);

      let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
      let list: IAudioSessionEnumerator = mgr.GetSessionEnumerator()?;
      let sessions = list.GetCount()?;

      for idx in 0..sessions {
        let ctrl = list.GetSession(idx)?;
        let ctrl2: IAudioSessionControl2 = ctrl.cast()?;
        let process_id = ctrl2.GetProcessId()?;
        let process_name = match get_process_name(process_id) {
          Some(n) => n,
          None => ctrl2.GetDisplayName()?.to_string()?,
        };

        // Skip system-sounds session
        if process_name.contains("AudioSrv") {
          continue;
        }

        let is_running = ctrl.GetState()? == AudioSessionStateActive;

        all_processes.push(AudioProcess {
          process_name,
          process_id,
          device_id: device_id.clone(),
          device_name: device_name.clone(),
          is_running,
        });
      }
    }

    Ok(all_processes)
  }
}

fn get_all_audio_devices() -> Result<Vec<AudioDevice>> {
  unsafe {
    let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    let device_collection: IMMDeviceCollection =
      enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;

    let device_count = device_collection.GetCount()?;
    let mut devices = Vec::new();

    // Get default devices for comparison
    let default_comm_device_id = enumerator
      .GetDefaultAudioEndpoint(eCapture, eCommunications)
      .and_then(|d| d.GetId())
      .and_then(|id| Ok(id.to_string().unwrap_or_default()))
      .ok();
    let default_console_device_id = enumerator
      .GetDefaultAudioEndpoint(eCapture, eConsole)
      .and_then(|d| d.GetId())
      .and_then(|id| Ok(id.to_string().unwrap_or_default()))
      .ok();

    for i in 0..device_count {
      let device: IMMDevice = device_collection.Item(i)?;

      let device_id_pwstr = device.GetId()?;
      let device_id = device_id_pwstr.to_string()?;
      let device_name = format!("Audio Device {}", i);

      let is_default_communications = default_comm_device_id.as_ref() == Some(&device_id);
      let is_default_console = default_console_device_id.as_ref() == Some(&device_id);

      let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
      let (has_active_sessions, _) = get_mgr_audio_session_running_status(&mgr)?;

      devices.push(AudioDevice {
        device_id,
        device_name,
        is_default_communications,
        is_default_console,
        has_active_sessions,
      });
    }

    Ok(devices)
  }
}

// Debug version with logging
#[napi]
pub fn debug_is_process_actively_using_microphone(pid: u32) -> NapiResult<String> {
  unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;
  };

  let mut debug_info = format!("Checking PID {}\n", pid);

  unsafe {
    let enumerator: IMMDeviceEnumerator =
      match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
        Ok(e) => e,
        Err(e) => {
          debug_info.push_str(&format!("Failed to create enumerator: {}\n", e.message()));
          return Ok(debug_info);
        }
      };

    // Check all capture devices, not just the default communications device
    let device_collection: IMMDeviceCollection =
      match enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) {
        Ok(collection) => collection,
        Err(e) => {
          debug_info.push_str(&format!("Failed to enumerate endpoints: {}\n", e.message()));
          return Ok(debug_info);
        }
      };

    let device_count = match device_collection.GetCount() {
      Ok(count) => count,
      Err(e) => {
        debug_info.push_str(&format!("Failed to get device count: {}\n", e.message()));
        return Ok(debug_info);
      }
    };

    debug_info.push_str(&format!("Found {} capture devices\n", device_count));

    for i in 0..device_count {
      let device: IMMDevice = match device_collection.Item(i) {
        Ok(d) => d,
        Err(e) => {
          debug_info.push_str(&format!("Failed to get device {}: {}\n", i, e.message()));
          continue;
        }
      };

      let device_id = match device.GetId() {
        Ok(id) => match id.to_string() {
          Ok(s) => s,
          Err(e) => {
            debug_info.push_str(&format!("Failed to convert device ID to string: {}\n", e));
            continue;
          }
        },
        Err(e) => {
          debug_info.push_str(&format!("Failed to get device ID: {}\n", e.message()));
          continue;
        }
      };

      debug_info.push_str(&format!("Device {}: {}\n", i, device_id));

      let mgr: IAudioSessionManager2 = match device.Activate(CLSCTX_ALL, None) {
        Ok(mgr) => mgr,
        Err(e) => {
          debug_info.push_str(&format!(
            "Failed to activate device {}: {}\n",
            i,
            e.message()
          ));
          continue;
        }
      };

      let list: IAudioSessionEnumerator = match mgr.GetSessionEnumerator() {
        Ok(l) => l,
        Err(e) => {
          debug_info.push_str(&format!(
            "Failed to get session enumerator for device {}: {}\n",
            i,
            e.message()
          ));
          continue;
        }
      };

      let session_count = match list.GetCount() {
        Ok(c) => c,
        Err(e) => {
          debug_info.push_str(&format!(
            "Failed to get session count for device {}: {}\n",
            i,
            e.message()
          ));
          continue;
        }
      };

      debug_info.push_str(&format!("  Device {} has {} sessions\n", i, session_count));

      for idx in 0..session_count {
        let ctrl: IAudioSessionControl = match list.GetSession(idx) {
          Ok(c) => c,
          Err(e) => {
            debug_info.push_str(&format!(
              "  Failed to get session {} for device {}: {}\n",
              idx,
              i,
              e.message()
            ));
            continue;
          }
        };

        let state = match ctrl.GetState() {
          Ok(s) => s,
          Err(e) => {
            debug_info.push_str(&format!(
              "  Failed to get state for session {} on device {}: {}\n",
              idx,
              i,
              e.message()
            ));
            continue;
          }
        };

        let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
          Ok(c) => c,
          Err(e) => {
            debug_info.push_str(&format!(
              "  Failed to cast to IAudioSessionControl2 for session {} on device {}: {}\n",
              idx,
              i,
              e.message()
            ));
            continue;
          }
        };

        let session_pid = match ctrl2.GetProcessId() {
          Ok(p) => p,
          Err(e) => {
            debug_info.push_str(&format!(
              "  Failed to get process ID for session {} on device {}: {}\n",
              idx,
              i,
              e.message()
            ));
            continue;
          }
        };

        let process_name =
          get_process_name(session_pid).unwrap_or_else(|| match ctrl2.GetDisplayName() {
            Ok(name) => name
              .to_string()
              .unwrap_or_else(|_| format!("PID_{}", session_pid)),
            Err(_) => format!("PID_{}", session_pid),
          });

        let is_active = state == AudioSessionStateActive;
        debug_info.push_str(&format!(
          "    Session {}: PID={}, Name={}, State={:?}, Active={}\n",
          idx, session_pid, process_name, state, is_active
        ));

        if session_pid == pid {
          debug_info.push_str(&format!(
            "    *** FOUND TARGET PID {} with state {:?} (active={})\n",
            pid, state, is_active
          ));
          if is_active {
            debug_info.push_str("    *** RETURNING TRUE\n");
            return Ok(debug_info);
          }
        }
      }
    }

    debug_info.push_str(&format!("PID {} not found in any active sessions\n", pid));
    Ok(debug_info)
  }
}

#[napi]
pub fn get_active_audio_processes() -> NapiResult<Vec<AudioProcess>> {
  unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;
  };

  let result = get_all_audio_processes()
    .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;

  // Filter to only return active/running processes
  let active_processes = result.into_iter().filter(|p| p.is_running).collect();
  Ok(active_processes)
}

// Helper function used by screen_capture_kit.rs
pub fn is_process_actively_using_microphone(pid: u32) -> NapiResult<bool> {
  unsafe {
    CoInitializeEx(None, COINIT_MULTITHREADED)
      .ok()
      .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;
  };

  let result = get_all_audio_processes()
    .map_err(|err| napi::Error::new(napi::Status::GenericFailure, err.message()))?;

  // Check if the PID exists in the list of active processes
  let is_active = result
    .iter()
    .any(|process| process.process_id == pid && process.is_running);

  Ok(is_active)
}

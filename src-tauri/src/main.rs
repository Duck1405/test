#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod screen;
mod transport;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, Serialize)]
struct DeviceState {
    phase: String,
    port: String,
    error: String,
    capabilities: Vec<String>,
    task_phase: String,
    task_id: String,
    mode: String,
    message: String,
    frames: u64,
}
#[derive(Clone, Serialize)]
struct LogEntry {
    time: u64,
    source: String,
    message: String,
}
enum Command {
    Task {
        id: String,
        mode: String,
        text: String,
    },
}
#[derive(Clone)]
struct Session {
    snapshot: Arc<Mutex<DeviceState>>,
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    cancel: Arc<AtomicBool>,
    task_cancel: Arc<AtomicBool>,
    indicators_ready: Arc<AtomicBool>,
    sender: Arc<Mutex<Option<mpsc::Sender<Command>>>>,
}
impl Session {
    fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(DeviceState {
                phase: "disconnected".into(),
                port: String::new(),
                error: String::new(),
                capabilities: vec![],
                task_phase: "idle".into(),
                task_id: String::new(),
                mode: "guide".into(),
                message: String::new(),
                frames: 0,
            })),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            cancel: Arc::new(AtomicBool::new(false)),
            task_cancel: Arc::new(AtomicBool::new(true)),
            indicators_ready: Arc::new(AtomicBool::new(false)),
            sender: Arc::new(Mutex::new(None)),
        }
    }
    fn log(&self, source: &str, message: impl AsRef<str>) {
        let mut logs = self.logs.lock().unwrap();
        logs.push_back(LogEntry {
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            source: source.into(),
            message: message.as_ref().chars().take(500).collect(),
        });
        while logs.len() > 400 {
            logs.pop_front();
        }
    }
    fn stop(&self, reason: &str) {
        let changed = {
            let mut s = self.snapshot.lock().unwrap();
            if s.task_phase == "active" || s.task_phase == "sending" {
                self.task_cancel.store(true, Ordering::SeqCst);
                self.indicators_ready.store(false, Ordering::SeqCst);
                s.task_phase = "stopping".into();
                s.message = reason.into();
                true
            } else {
                false
            }
        };
        if changed {
            self.log("laptop", reason);
        }
    }
    fn active(&self) -> bool {
        self.snapshot.lock().unwrap().task_phase == "active"
            && !self.task_cancel.load(Ordering::SeqCst)
    }
}

fn main_only(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("This control is only available in Bob's main widget.".into())
    }
}
#[tauri::command]
fn list_ports() -> Result<Vec<transport::Port>, String> {
    transport::ports()
}
#[tauri::command]
fn get_state(session: tauri::State<Session>) -> DeviceState {
    session.snapshot.lock().unwrap().clone()
}
#[tauri::command]
fn get_logs(session: tauri::State<Session>) -> Vec<LogEntry> {
    session.logs.lock().unwrap().iter().cloned().collect()
}
#[tauri::command]
async fn diagnose_usb(
    window: tauri::WebviewWindow,
    session: tauri::State<'_, Session>,
) -> Result<(), String> {
    main_only(&window)?;
    session.log("laptop", transport::diagnostic());
    Ok(())
}

fn valid_reply(obj: &Value, kind: &str, nonce: &str) -> bool {
    obj["type"] == kind
        && obj["device"] == "device-companion"
        && obj["protocol"] == 1
        && obj["nonce"] == nonce
}
fn cancelled(session: &Session, task: bool) -> bool {
    session.cancel.load(Ordering::SeqCst) || (task && session.task_cancel.load(Ordering::SeqCst))
}
fn exchange(
    port: &mut dyn transport::Transport,
    session: &Session,
    mut packet: Value,
    reply: &str,
    seconds: u64,
    task: bool,
) -> Result<Option<Value>, String> {
    let nonce = uuid::Uuid::new_v4().to_string();
    packet["nonce"] = json!(nonce);
    packet["protocol"] = json!(1);
    let bytes = format!("{packet}\n");
    let started = Instant::now();
    let mut next = Duration::ZERO;
    let mut pending = Vec::new();
    let mut buf = [0; 1024];
    while started.elapsed() < Duration::from_secs(seconds) && !cancelled(session, task) {
        if started.elapsed() >= next {
            port.write_all(bytes.as_bytes())
                .map_err(|_| "USB write failed. Reconnect the device.")?;
            next = started.elapsed() + Duration::from_secs(1);
        }
        match port.read(&mut buf) {
            Ok(0) => return Err("The USB device disconnected.".into()),
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=end).collect();
                    let Ok(obj) = serde_json::from_slice::<Value>(&line) else {
                        continue;
                    };
                    if obj["type"] == "log"
                        && obj["device"] == "device-companion"
                        && obj["protocol"] == 1
                    {
                        if let Some(message) = obj["message"].as_str() {
                            session.log("uno", message);
                        }
                    }
                    if valid_reply(&obj, "error", &nonce) {
                        return Err(obj["message"]
                            .as_str()
                            .unwrap_or("Device rejected the request.")
                            .chars()
                            .take(250)
                            .collect());
                    }
                    if valid_reply(&obj, reply, &nonce) {
                        if packet.get("task_id").is_some() && obj["task_id"] != packet["task_id"] {
                            continue;
                        }
                        if packet.get("frame_id").is_some() && obj["frame_id"] != packet["frame_id"]
                        {
                            continue;
                        }
                        return Ok(Some(obj));
                    }
                }
                if pending.len() > 8192 {
                    return Err("Device sent an oversized message.".into());
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(_) => return Err("The USB device disconnected.".into()),
        }
    }
    if cancelled(session, task) {
        Ok(None)
    } else {
        Err("Device response timed out. Check the USB connection and UNO Q service.".into())
    }
}

fn send_frame(
    port: &mut dyn transport::Transport,
    session: &Session,
    id: &str,
    jpeg: &[u8],
) -> Result<(), String> {
    let frame = uuid::Uuid::new_v4().to_string();
    if exchange(
        port,
        session,
        json!({"type":"screen_begin","task_id":id,"frame_id":frame,"size":jpeg.len()}),
        "screen_ready",
        3,
        true,
    )?
    .is_none()
    {
        return Ok(());
    }
    for (index, chunk) in jpeg.chunks(768).enumerate() {
        if cancelled(session, true) {
            return Ok(());
        }
        let packet = json!({"type":"screen_chunk","protocol":1,"nonce":frame,"task_id":id,"frame_id":frame,"index":index,"data":STANDARD.encode(chunk)});
        port.write_all(format!("{packet}\n").as_bytes())
            .map_err(|_| "Screen transfer failed. Sharing stopped.")?;
    }
    if exchange(port,session,json!({"type":"screen_end","task_id":id,"frame_id":frame,"sha256":format!("{:x}",Sha256::digest(jpeg))}),"screen_received",3,true)?.is_some() && !cancelled(session,true) {
        session.snapshot.lock().unwrap().frames+=1;
        session.log("laptop",format!("Screen frame acknowledged: {} JPEG bytes.",jpeg.len()));
    }
    Ok(())
}

fn run_device(
    name: &str,
    session: &Session,
    receiver: mpsc::Receiver<Command>,
) -> Result<(), String> {
    let mut port = transport::open(name)?;
    session.log("laptop", format!("Opened {name}; verifying device."));
    let Some(hello) = exchange(
        port.as_mut(),
        session,
        json!({"type":"hello"}),
        "hello_ack",
        5,
        false,
    )?
    else {
        return Ok(());
    };
    {
        let mut state = session.snapshot.lock().unwrap();
        if session.cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        state.phase = "connected".into();
        state.capabilities = hello["capabilities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
    }
    session.log(
        "laptop",
        "Device verified. Task and screen sharing require the UNO Q receiver service.",
    );
    let mut active_id: Option<String> = None;
    let mut heartbeat = Instant::now();
    let mut frame_time = Instant::now() - Duration::from_secs(4);
    while !session.cancel.load(Ordering::SeqCst) {
        let stopping = session.snapshot.lock().unwrap().task_phase == "stopping";
        if stopping {
            if let Some(id) = active_id.take() {
                // Stop locally first; notify the board if the cable is still available.
                let _ = exchange(
                    port.as_mut(),
                    session,
                    json!({"type":"stop_task","task_id":id}),
                    "task_stopped",
                    2,
                    false,
                );
                heartbeat = Instant::now();
            }
            // A new task cannot reset cancellation until the old worker has
            // finished its transfer/stop and discarded any unsent old request.
            while receiver.try_recv().is_ok() {}
            session.snapshot.lock().unwrap().task_phase = "stopped".into();
        }
        if let Ok(Command::Task { id, mode, text }) = receiver.try_recv() {
            if cancelled(session, true) {
                continue;
            }
            active_id = Some(id.clone());
            match exchange(
                port.as_mut(),
                session,
                json!({"type":"task","task_id":id,"mode":mode,"text":text}),
                "task_received",
                5,
                true,
            ) {
                Ok(Some(_)) => {
                    let mut state = session.snapshot.lock().unwrap();
                    if !cancelled(session, true) {
                        state.task_phase = "active".into();
                        state.message="Received by UNO Q. Starting primary-screen sharing. AI is not installed yet.".into();
                    }
                    drop(state);
                    session.log(
                        "laptop",
                        "UNO Q acknowledged the task. Waiting for the visible sharing indicators.",
                    );
                    frame_time = Instant::now() - Duration::from_secs(4);
                }
                Ok(None) => {}
                Err(error) => {
                    session.log("laptop", &error);
                    session.stop(&error);
                }
            }
            heartbeat = Instant::now();
        }
        if session.active()
            && session.indicators_ready.load(Ordering::SeqCst)
            && frame_time.elapsed() >= Duration::from_secs(3)
        {
            let result = screen::capture().and_then(|jpeg| {
                if !cancelled(session, true) {
                    if let Some(id) = active_id.as_ref() {
                        send_frame(port.as_mut(), session, id, &jpeg)?;
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                session.log("laptop", &error);
                session.stop(&error);
            }
            frame_time = Instant::now();
            heartbeat = Instant::now();
        }
        if heartbeat.elapsed() >= Duration::from_secs(1) {
            let task_id = if session.active() {
                active_id.as_deref()
            } else {
                None
            };
            exchange(
                port.as_mut(),
                session,
                json!({"type":"ping","task_id":task_id}),
                "pong",
                3,
                false,
            )?;
            heartbeat = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

#[tauri::command]
async fn connect_device(
    window: tauri::WebviewWindow,
    port: String,
    session: tauri::State<'_, Session>,
) -> Result<(), String> {
    main_only(&window)?;
    if !transport::ports()?.iter().any(|p| p.name == port) {
        return Err("That device is no longer available. Refresh and try again.".into());
    }
    let (sender, receiver) = mpsc::channel();
    {
        let mut state = session.snapshot.lock().unwrap();
        if state.phase != "disconnected" {
            return Err("A device connection is already active.".into());
        }
        session.cancel.store(false, Ordering::SeqCst);
        session.task_cancel.store(true, Ordering::SeqCst);
        state.phase = "connecting".into();
        state.port = port.clone();
        state.error.clear();
        state.capabilities.clear();
        state.task_phase = "idle".into();
        state.message.clear();
        state.frames = 0;
        *session.sender.lock().unwrap() = Some(sender);
    }
    let owned = (*session).clone();
    std::thread::spawn(move || {
        let result = run_device(&port, &owned, receiver);
        owned.stop("Device disconnected. Screen sharing stopped.");
        let error = if owned.cancel.load(Ordering::SeqCst) {
            String::new()
        } else {
            result.err().unwrap_or_default()
        };
        owned.log(
            "laptop",
            if error.is_empty() {
                "USB connection closed."
            } else {
                &error
            },
        );
        *owned.sender.lock().unwrap() = None;
        let mut state = owned.snapshot.lock().unwrap();
        state.phase = "disconnected".into();
        if state.task_phase == "stopping" {
            state.task_phase = "stopped".into();
        }
        state.error = error;
    });
    Ok(())
}
#[tauri::command]
fn disconnect_device(
    window: tauri::WebviewWindow,
    session: tauri::State<Session>,
) -> Result<(), String> {
    main_only(&window)?;
    session.stop("Disconnected by you. Screen sharing stopped.");
    let mut state = session.snapshot.lock().unwrap();
    if state.phase != "disconnected" {
        session.cancel.store(true, Ordering::SeqCst);
        state.phase = "disconnecting".into();
    }
    Ok(())
}
#[tauri::command]
fn start_task(
    window: tauri::WebviewWindow,
    text: String,
    mode: String,
    session: tauri::State<Session>,
) -> Result<String, String> {
    main_only(&window)?;
    if text.trim().is_empty() || text.len() > 4000 {
        return Err("Enter a task of up to 4,000 UTF-8 bytes.".into());
    }
    if mode != "do" && mode != "guide" {
        return Err("Choose Do it for me or Guide me.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut state = session.snapshot.lock().unwrap();
        if state.phase != "connected" {
            return Err("Connect the UNO Q first.".into());
        }
        if !["tasks_v1", "screen_v1"]
            .iter()
            .all(|required| state.capabilities.iter().any(|c| c == required))
        {
            return Err("Upload the Bob receiver service to the UNO Q first. This device only supports connection checks.".into());
        }
        if matches!(state.task_phase.as_str(), "active" | "sending" | "stopping") {
            return Err("Stop the current session before sending another task.".into());
        }
        session.task_cancel.store(false, Ordering::SeqCst);
        session.indicators_ready.store(false, Ordering::SeqCst);
        state.task_phase = "sending".into();
        state.task_id = id.clone();
        state.mode = mode.clone();
        state.message = "Sending your task to UNO Q…".into();
        state.frames = 0;
    }
    let result = session
        .sender
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("USB connection is closed.")
        .and_then(|tx| {
            tx.send(Command::Task {
                id: id.clone(),
                mode,
                text,
            })
            .map_err(|_| "USB connection is closed.")
        });
    if let Err(error) = result {
        session.stop(error);
        return Err(error.into());
    }
    session.log(
        "laptop",
        format!("Task submitted ({id}). Waiting for UNO Q receipt."),
    );
    Ok(id)
}
#[tauri::command]
fn stop_task(session: tauri::State<Session>) {
    session.stop("Stopped by you. Screen sharing is off.");
}

fn show_indicators(app: &tauri::AppHandle, session: &Session, show: bool) -> Result<(), String> {
    if show && !session.active() {
        return Ok(());
    }
    for name in ["sharing-border", "sharing-stop"] {
        let window = app
            .get_webview_window(name)
            .ok_or("Screen indicator window is missing.")?;
        if show {
            window.show().map_err(|e| e.to_string())?;
        } else {
            window.hide().map_err(|e| e.to_string())?;
        }
    }
    session
        .indicators_ready
        .store(show && session.active(), Ordering::SeqCst);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(Session::new())
        .setup(|app| {
            let window = app.get_webview_window("main").ok_or("Main widget is missing")?;
            if let Some(monitor) = window.primary_monitor()? {
                let scale = monitor.scale_factor();
                let origin = monitor.position();
                let size = monitor.size();
                window.set_position(PhysicalPosition::new(
                    (origin.x + size.width as i32 - (378.0 * scale) as i32).max(origin.x),
                    (origin.y + size.height as i32 - (365.0 * scale) as i32).max(origin.y),
                ))?;
                let border = WebviewWindowBuilder::new(app, "sharing-border", WebviewUrl::App("index.html?view=overlay".into()))
                    .title("Bob — primary screen sharing")
                    .decorations(false).transparent(true).shadow(false)
                    .always_on_top(true).skip_taskbar(true).focused(false)
                    .focusable(false).resizable(false).visible(false).build()?;
                border.set_position(*origin)?;
                border.set_size(*size)?;
                border.set_ignore_cursor_events(true)?;
                let stop = WebviewWindowBuilder::new(app, "sharing-stop", WebviewUrl::App("index.html?view=stop".into()))
                    .title("Bob — stop screen sharing")
                    .inner_size(310.0, 46.0).decorations(false).transparent(true)
                    .shadow(false).always_on_top(true).skip_taskbar(true)
                    .focused(false).resizable(false).visible(false).build()?;
                stop.set_position(PhysicalPosition::new(
                    origin.x + (size.width as i32 - (310.0 * scale) as i32) / 2,
                    origin.y + (12.0 * scale) as i32,
                ))?;
            }
            window.show()?;
            let handle = app.handle().clone();
            let session = app.state::<Session>().inner().clone();
            session.log("laptop", "Bob 0.3 started. No screen is captured until a task is acknowledged and sharing indicators are visible.");
            std::thread::spawn(move || {
                let mut shown = false;
                loop {
                    let running = {
                        let state = session.snapshot.lock().unwrap();
                        state.task_phase == "active" || state.task_phase == "sending"
                    };
                    if running && screen::escape_down() {
                        session.stop("Escape pressed. Screen sharing is off.");
                    }
                    let active = session.active();
                    if active != shown {
                        shown = active;
                        let handle2 = handle.clone();
                        let session2 = session.clone();
                        let _ = handle.run_on_main_thread(move || {
                            if let Err(error) = show_indicators(&handle2, &session2, active) {
                                session2.log("laptop", &error);
                                session2.stop("Couldn’t display the sharing indicators. Session stopped.");
                                let _ = show_indicators(&handle2, &session2, false);
                            }
                        });
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let session = window.state::<Session>();
                if window.label() == "main" {
                    session.stop("Bob closed. Screen sharing stopped.");
                    session.cancel.store(true, Ordering::SeqCst);
                    window.app_handle().exit(0);
                } else if window.label().starts_with("sharing-") {
                    api.prevent_close();
                    session.stop("Sharing indicator closed. Screen sharing stopped.");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_ports, get_state, get_logs, diagnose_usb, connect_device,
            disconnect_device, start_task, stop_task
        ])
        .run(tauri::generate_context!())
        .expect("Could not start Bob the Butler");
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeTransport {
        pending: VecDeque<u8>,
        writes: usize,
    }
    impl std::io::Write for FakeTransport {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            let packet: Value = serde_json::from_slice(data).unwrap();
            let reply = match packet["type"].as_str().unwrap() {
                "hello" => "hello_ack",
                "screen_begin" => "screen_ready",
                "screen_end" => "screen_received",
                _ => return Ok(data.len()),
            };
            let log = json!({"type":"log","device":"device-companion","protocol":1,"message":"Receiver ready"});
            let response = json!({"type":reply,"device":"device-companion","protocol":1,"nonce":packet["nonce"],"task_id":packet["task_id"],"frame_id":packet["frame_id"]});
            self.pending.extend(format!("{log}\n{response}\n").bytes());
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl std::io::Read for FakeTransport {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(7).min(self.pending.len());
            if n == 0 {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            for b in &mut buf[..n] {
                *b = self.pending.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    #[test]
    fn fragmented_replies_and_device_logs_are_read() {
        let s = Session::new();
        let mut port = FakeTransport {
            pending: VecDeque::new(),
            writes: 0,
        };
        let result = exchange(
            &mut port,
            &s,
            json!({"type":"hello"}),
            "hello_ack",
            1,
            false,
        )
        .unwrap();
        assert!(result.is_some());
        assert_eq!(s.logs.lock().unwrap()[0].source, "uno");
    }
    #[test]
    fn screen_transfer_requires_matching_receipt_and_stops_before_io() {
        let s = Session::new();
        s.task_cancel.store(false, Ordering::SeqCst);
        let mut port = FakeTransport {
            pending: VecDeque::new(),
            writes: 0,
        };
        send_frame(&mut port, &s, "task-1", b"fake-jpeg").unwrap();
        assert_eq!(s.snapshot.lock().unwrap().frames, 1);
        s.task_cancel.store(true, Ordering::SeqCst);
        let count = port.writes;
        send_frame(&mut port, &s, "task-1", b"late-frame").unwrap();
        assert_eq!(port.writes, count);
        assert_eq!(s.snapshot.lock().unwrap().frames, 1);
    }
    #[test]
    fn validates_reply_identity_nonce_and_type() {
        let obj =
            json!({"type":"hello_ack","device":"device-companion","protocol":1,"nonce":"abc"});
        assert!(valid_reply(&obj, "hello_ack", "abc"));
        assert!(!valid_reply(&obj, "hello_ack", "other"));
        assert!(!valid_reply(&obj, "pong", "abc"));
        assert!(!valid_reply(&json!(null), "hello_ack", "abc"));
    }
    #[test]
    fn stop_is_local_and_idempotent() {
        let s = Session::new();
        s.task_cancel.store(false, Ordering::SeqCst);
        s.indicators_ready.store(true, Ordering::SeqCst);
        s.snapshot.lock().unwrap().task_phase = "active".into();
        s.stop("Escape pressed");
        assert!(!s.active());
        assert!(s.task_cancel.load(Ordering::SeqCst));
        assert!(!s.indicators_ready.load(Ordering::SeqCst));
        s.stop("again");
        assert_eq!(s.logs.lock().unwrap().len(), 1);
    }
    #[test]
    fn logs_are_bounded() {
        let s = Session::new();
        for _ in 0..500 {
            s.log("laptop", "test");
        }
        assert_eq!(s.logs.lock().unwrap().len(), 400);
    }
}

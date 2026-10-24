#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, PhysicalPosition};

#[derive(Clone, Serialize)]
struct DeviceState {
    phase: String,
    port: String,
    error: String,
}

struct Session {
    snapshot: Arc<Mutex<DeviceState>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
struct Port {
    name: String,
    label: String,
    usb: bool,
}

#[tauri::command]
fn list_ports() -> Result<Vec<Port>, String> {
    let mut ports: Vec<Port> = serialport::available_ports().map_err(|_| "Couldn’t scan COM ports. Try again.")?
        .into_iter().map(|p| {
            let (usb, description) = match p.port_type {
                serialport::SerialPortType::UsbPort(info) => (true, info.product.unwrap_or_else(|| "USB device".into())),
                _ => (false, "Serial port".into()),
            };
            Port { label: format!("{} · {}", p.port_name, description), name: p.port_name, usb }
        }).collect();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ports)
}

#[tauri::command]
fn get_state(session: tauri::State<Session>) -> DeviceState {
    session.snapshot.lock().unwrap().clone()
}

fn valid_reply(raw: &[u8], kind: &str, nonce: &str) -> bool {
    let Ok(obj) = serde_json::from_slice::<Value>(raw) else { return false; };
    obj["type"] == kind && obj["device"] == "device-companion"
        && obj["protocol"] == 1 && obj["nonce"] == nonce
}

fn exchange(port: &mut dyn serialport::SerialPort, kind: &str, reply: &str,
            deadline: Duration, cancel: &AtomicBool) -> Result<bool, String> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let packet = format!("{}\n", json!({"type":kind,"protocol":1,"nonce":nonce}));
    let start = Instant::now();
    let mut next_send = Duration::ZERO;
    let mut pending = Vec::new();
    let mut buf = [0u8; 256];
    while start.elapsed() < deadline && !cancel.load(Ordering::Relaxed) {
        if start.elapsed() >= next_send {
            port.write_all(packet.as_bytes()).map_err(|_| "Device disconnected. Plug it in and try again.")?;
            next_send = start.elapsed() + Duration::from_secs(1);
        }
        match port.read(&mut buf) {
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                    let raw: Vec<u8> = pending.drain(..=end).collect();
                    if valid_reply(&raw, reply, &nonce) { return Ok(true); }
                }
                if pending.len() > 4096 { pending.clear(); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return Err("Device disconnected. Plug it in and try again.".into()),
        }
    }
    Ok(false)
}

fn run_device(name: &str, snapshot: &Mutex<DeviceState>, cancel: &AtomicBool) -> Result<(), String> {
    let mut port = serialport::new(name, 115200).timeout(Duration::from_millis(150))
        .dtr_on_open(false).open()
        .map_err(|_| format!("Can’t open {name}. Close other serial apps and try again."))?;
    let _ = port.write_request_to_send(false);
    if !exchange(port.as_mut(), "hello", "hello_ack", Duration::from_secs(5), cancel)? {
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        return Err("Wrong device or missing firmware. Choose another port and try again.".into());
    }
    {
        let mut state = snapshot.lock().unwrap();
        if cancel.load(Ordering::Relaxed) { return Ok(()); }
        state.phase = "connected".into();
    }
    while !cancel.load(Ordering::Relaxed) {
        // Short waits make Disconnect responsive without blocking the UI thread.
        for _ in 0..10 {
            if cancel.load(Ordering::Relaxed) { return Ok(()); }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !exchange(port.as_mut(), "ping", "pong", Duration::from_secs(3), cancel)? {
            if cancel.load(Ordering::Relaxed) { return Ok(()); }
            return Err("Your device stopped responding. Reconnect it and try again.".into());
        }
    }
    Ok(())
}

#[tauri::command]
fn connect_device(port: String, session: tauri::State<Session>) -> Result<(), String> {
    if !serialport::available_ports().map_err(|e| e.to_string())?.iter().any(|p| p.port_name == port) {
        return Err("That port is no longer available. Refresh ports and try again.".into());
    }
    {
        let mut state = session.snapshot.lock().unwrap();
        if state.phase != "disconnected" { return Err("A device connection is already active.".into()); }
        session.cancel.store(false, Ordering::Relaxed);
        *state = DeviceState { phase: "connecting".into(), port: port.clone(), error: String::new() };
    }
    let snapshot = session.snapshot.clone();
    let cancel = session.cancel.clone();
    std::thread::spawn(move || {
        let result = run_device(&port, &snapshot, &cancel);
        let mut state = snapshot.lock().unwrap();
        state.phase = "disconnected".into();
        state.error = if cancel.load(Ordering::Relaxed) { String::new() } else { result.err().unwrap_or_default() };
    });
    Ok(())
}

#[tauri::command]
fn disconnect_device(session: tauri::State<Session>) {
    let mut state = session.snapshot.lock().unwrap();
    if state.phase != "disconnected" {
        session.cancel.store(true, Ordering::Relaxed);
        state.phase = "disconnecting".into();
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Session {
            snapshot: Arc::new(Mutex::new(DeviceState { phase: "disconnected".into(), port: String::new(), error: String::new() })),
            cancel: Arc::new(AtomicBool::new(false)),
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Some(monitor) = window.primary_monitor()? {
                    let scale = monitor.scale_factor();
                    let origin = monitor.position();
                    let size = monitor.size();
                    let x = origin.x + size.width as i32 - (378.0 * scale) as i32;
                    let y = origin.y + size.height as i32 - (365.0 * scale) as i32;
                    window.set_position(PhysicalPosition::new(x.max(origin.x), y.max(origin.y)))?;
                }
                window.show()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![list_ports, get_state, connect_device, disconnect_device])
        .run(tauri::generate_context!())
        .expect("Could not start Bob the Butler");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_matching_device_and_nonce() {
        let raw = br#"{"type":"hello_ack","device":"device-companion","protocol":1,"nonce":"abc"}"#;
        assert!(valid_reply(raw, "hello_ack", "abc"));
        assert!(!valid_reply(raw, "hello_ack", "other"));
        assert!(!valid_reply(raw, "pong", "abc"));
    }
    #[test]
    fn rejects_unrelated_device_and_garbage() {
        for raw in [b"boot log".as_slice(), b"null", b"[]", br#"{"type":"hello_ack","device":"other","protocol":1,"nonce":"abc"}"#] {
            assert!(!valid_reply(raw, "hello_ack", "abc"));
        }
    }
}

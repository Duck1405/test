# Bob the Butler

A compact Windows desktop widget built with **React, TypeScript and Tauri 2**. The native Rust backend owns USB serial connections; no browser serial permission or Python installation is needed.

## Download and run

Download `DeviceCompanion.exe` from the repository using GitHub’s **Download raw file** button, then open it. The executable keeps its previous filename for compatibility; the app is named Bob the Butler. Windows requires the Microsoft Edge WebView2 runtime, normally present on Windows 10/11. A board-specific USB serial driver may also be required.

Bob opens as a light, rounded 360 × 300 widget near the bottom-right of the primary monitor. Drag the header to move it, or use × to close it.

Plug in your device. Bob checks USB serial candidates automatically at **115200 baud**. If needed, select a COM port and press the green arrow. **Refresh ports** rescans and retries. A matching device response opens the connected screen; wrong devices and unavailable ports show a retry message. Heartbeats return the app to its connection screen when the device stops responding. Close other serial monitors before connecting. Some adapters may reset a board when their port opens.

## Automated Windows build

`.github/workflows/windows.yml` runs on application changes pushed to `master`. It builds and checks the Rust protocol, builds the React frontend, creates the Windows executable and NSIS installer, and uploads both as the **Bob-the-Butler-Windows** workflow artifact. On a successful push build it also commits the newly built portable executable to the repository root as `DeviceCompanion.exe`. The binary-only commit does not retrigger the build. Repository branch rules must allow the workflow token to push that binary commit.

## Build locally on Windows

Install Node.js 22+, pnpm 11.25, stable Rust with the MSVC toolchain, Microsoft C++ Build Tools with Windows SDK, and Microsoft Edge WebView2. Then run:

```powershell
.\build.ps1
```

The script runs dependency installation, Rust protocol checks and the Tauri release build. It copies the portable executable into the project root. The installer is under `src-tauri/target/release/bundle/nsis/`.

Development: `pnpm install --frozen-lockfile`, then `pnpm tauri dev`.
Frontend-only preview: `pnpm dev` (device controls require the native app).
Frontend type check and production build: `pnpm build`.
Protocol checks: `cargo test --manifest-path src-tauri/Cargo.toml`.

## Firmware protocol

The board must implement newline-delimited JSON:

- Host sends `{"type":"hello","protocol":1,"nonce":"<random>"}`.
- Device returns `{"type":"hello_ack","device":"device-companion","protocol":1,"nonce":"<same random>"}`.
- Host later sends `ping` with a new nonce; device responds with `pong` and the same identity, version and nonce.

Hello has a five-second deadline; heartbeat replies have a three-second deadline. Messages are retried once a second. This checks application compatibility, not cryptographic authentication.

For the existing ESP32, `firmware/companion_serial/companion_serial.ino` is a development responder. Install ArduinoJson 7, select your ESP32 board and upload it through Arduino IDE. It has not been compiled or flashed here. UNO Q Linux USB transport and a matching responder still need implementation and hardware validation.

## Scope

This milestone implements the React widget, serial connection, device verification, and disconnect handling. AI, task execution, screenshot transfer and physical-input cancellation are not implemented yet. macOS and Linux need separate builds and testing.

The previous Python/Tkinter prototype (`app.py`, `protocol.py`, `requirements.txt`, `tests/`) is retained as a reference. It is not bundled into the Tauri executable.

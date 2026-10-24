# Bob the Butler

A compact Windows desktop widget built with **React, TypeScript and Tauri 2**, with a Rust USB/session backend and a Python receiver prepared for the Arduino UNO Q.

## Run

Download `DeviceCompanion.exe` from the repository using GitHub's **Download raw file** button, then open it. The filename is retained for compatibility; the app is named Bob the Butler. Windows needs Microsoft Edge WebView2, normally present on Windows 10/11.

The light, rounded 360 × 300 widget opens near the bottom-right corner. Drag its header to move it. The device picker includes physically connected USB ADB boards and COM ports. For UNO Q, complete initial setup in Arduino App Lab and keep its ADB server running. Use **Debug → Check USB** for diagnostics.

The UNO Q receiver is in [`uno-q/`](uno-q/README.md). It has been installed on the Q and verified to start automatically after reboot. Both task modes, synthetic image transfer and Stop passed USB protocol checks; the AI model is not installed. A COM-only ESP32 with the old handshake sketch can connect but cannot start a task.

## Task and screen-sharing flow

1. Connect and verify the device. The service must advertise task and screen capabilities.
2. Choose **Do it for me** or **Guide me** inside the chat composer.
3. Enter a task and press Send. The UI explicitly states that sending starts primary-screen sharing.
4. Bob sends the task and mode to UNO Q and waits for its `task_received` acknowledgement.
5. Red corners/border and a small top-of-screen Stop / Esc popup appear. Only then does capture start.
6. Bob sends downscaled JPEG frames from the **primary monitor only**, up to 640 pixels wide and 32 KiB each. New frames are scheduled three seconds after the previous transfer; serial links may be slower. Each frame is acknowledged by the board.
7. Press **Escape anywhere**, click Stop in the popup/widget, disconnect, or close Bob to stop capture. Escape is checked by a native Windows monitor, independent of widget focus or board replies. In-flight bytes may finish reaching the board; a stop request and session lease prevent continuing the old session. Reconnecting does not resume sharing.

**Guide me:** a large blue overlay pointer marks the target. A pulsing blue circle means “click here.” You move/click the real cursor yourself; Bob observes your click near the target and acknowledges the step. The Debug view also has a manual **Step done** fallback. Guide mode does not inject input.

**Do it for me:** the same blue target appears briefly before Bob moves the real Windows cursor and, for a click action, sends one left click. Any physical keyboard or mouse input stops Auto. Escape, Stop, disconnect and closing Bob stop both modes. Auto is disabled if physical-input monitoring cannot start.

**AI inference is not enabled in the widget.** Pretrained benchmark models are staged on the Q; the command-planner training kit is in [`uno-q/ai/`](uno-q/ai/README.md). No target is generated from a normal task yet. For now, **Debug → pointer test** starts an explicit one-step test through UNO Q. Choose Guide/Auto, a target X/Y percentage of the primary screen, and Move/Click, then click the clearly labeled test button. Auto tests move/click the real mouse: select a harmless target before starting and leave the mouse still. Stop the session after testing. Default test action is Move.

The future model will call `Receiver.queue_action()` on UNO Q. Requests are tied to the current task and acknowledged screenshot, have unique IDs, and are processed once. Auto requires a fresh board permit and rejects targets based on screens older than ten seconds. Guide targets expire after thirty seconds. Only normalized primary-screen move and left-click actions are supported; no shell, typing, dragging or other buttons are exposed.

Debug is beside “Your device. Your control.” It opens bounded Laptop / UNO Q metadata logs inside the widget. No screenshot or prompt payloads are logged. The Q retains the latest frame in RAM only, and releases it when the session ends.

## Automated Windows build

`.github/workflows/windows.yml` runs on application or receiver changes pushed to `master`. It checks the Rust protocol/session helpers and Python receiver, builds the React frontend and Tauri Windows app, creates an NSIS installer, and uploads the Windows outputs as **Bob-the-Butler-Windows**. A successful push build also commits the new portable `DeviceCompanion.exe` to the root. Binary-only commits do not retrigger the workflow. Repository branch rules must allow that commit.

## Build locally

Install Node.js 22+, pnpm 11.25, stable Rust with the MSVC toolchain, Microsoft C++ Build Tools with Windows SDK, and WebView2. Run:

```powershell
.\build.ps1
```

Output: root `DeviceCompanion.exe`; installer under `src-tauri/target/release/bundle/nsis/`.

- Development: `pnpm install --frozen-lockfile`, then `pnpm tauri dev`.
- Browser preview: `pnpm dev` (USB/native controls require the desktop app).
- Frontend validation: `pnpm build`.
- Rust checks: `cargo test --manifest-path src-tauri/Cargo.toml`.
- Receiver checks: `python -m unittest discover -s uno-q -p "test_*.py" -v`.

macOS/Linux need platform implementations and testing for capture, overlays and Escape handling. This release targets Windows.

The original Python/Tkinter prototype (`app.py`, `protocol.py`, `requirements.txt`, `tests/`) is retained as a reference and is not bundled into the Tauri app.

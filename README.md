# Bob the Butler

Windows-first USB companion for the planned local AI assistant.

## Run

Open **DeviceCompanion.exe** in the project folder. The light 360 × 300 panel opens at the bottom-right of the primary screen, above the taskbar. It has rounded corners, a draggable Bob the Butler header, and a close button. Plug in the device: the app checks USB serial candidates automatically at 115200 baud. If it cannot find the device, select a COM port and press the green arrow. **Refresh ports** scans again and retries automatic detection.

A successful device handshake opens the connected screen. A wrong port, unavailable port, or missing firmware keeps you on the connection screen with a retry message. The app checks device responsiveness and returns to the connection screen if it loses contact. Close other serial monitors before connecting. Opening a USB serial adapter can reset some boards.

This is a portable executable with Python bundled, not an installer. A board-specific serial driver may still be required.

## Device firmware is required

Opening a port alone does not prove it is our device. The board must implement the newline-delimited JSON protocol in `protocol.py`:

- Host sends `{"type":"hello","protocol":1,"nonce":"<random>"}`.
- Device returns `{"type":"hello_ack","device":"device-companion","protocol":1,"nonce":"<same random>"}`.
- Host later sends `ping` with a new nonce; device responds with `pong` and the same identity, version, and nonce.

Hello has a five-second deadline; heartbeat replies have a three-second deadline. Messages are retried once a second. This is an application compatibility check, not cryptographic authentication.

For the existing ESP32, `firmware/companion_serial/companion_serial.ino` is a development responder. Install ArduinoJson 7 in Arduino IDE, select your ESP32 board, and upload the sketch. The sketch has not been compiled or flashed in this environment. UNO Q Linux-side USB transport and its responder still need implementation and hardware validation; an unconfigured board will not pass the check.

## Development

Install Python 3 with tkinter, then run:

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

The build writes `dist/DeviceCompanion.exe` and copies it to the project root for GitHub Desktop. Close the executable before rebuilding. The root executable is tracked by Git; build outputs and the virtual environment are ignored.

Run source with `.venv\Scripts\python.exe app.py`.

Run checks with `.venv\Scripts\python.exe -m unittest discover -s tests -v`.

This milestone implements device connection and verification screens. AI, task execution, screenshot transfer, and physical-input cancellation are not implemented yet.

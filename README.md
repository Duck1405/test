# Device Companion

Windows-first USB serial companion for the planned UNO Q desktop assistant.

## Run on Windows

Open `dist/DeviceCompanion.exe`, plug in your board, click **Refresh**, select its COM port and baud rate, then click **Connect**. The default is 115200 baud; match your firmware. Close Arduino Serial Monitor or other software using the port first. Click **Disconnect** to release the port.

This is a portable executable, not an installer. Python is bundled. A board-specific USB serial driver may still be required. No COM port appears unless the board exposes a serial interface; UNO Q Linux-side USB transport still needs to be configured and validated.

The app opens a serial connection and displays received text. It does not yet verify device identity, send tasks, run AI, capture the screen, or control input. Some USB serial adapters may reset their board when opening a port, even with DTR/RTS disabled.

## Development and build

Install Python 3 with tkinter, then run `powershell -ExecutionPolicy Bypass -File .\build.ps1`. Output: `dist/DeviceCompanion.exe`.

Run source with `.venv\Scripts\python.exe app.py`. Build outputs and the virtual environment are excluded from Git; publish executables as GitHub release assets.

## Next milestone

Define a versioned device handshake and implement a matching board service. Then add task messages, screenshot transport, cancellation and AI inference.

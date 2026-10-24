# UNO Q receiver (prepared for the next hardware session)

`bob_service.py` runs on the **Debian/Linux side of UNO Q**, not as a microcontroller sketch. It uses Python 3's standard library and binds only to `127.0.0.1:8765` on the board. The Windows app reaches that socket over a physical USB ADB transport. Wi-Fi ADB devices are not offered by Bob.

Nothing has been uploaded to a board yet. Once you connect the Q, we will verify its initial setup and USB access in Arduino App Lab, copy this folder to the board, and run `python3 bob_service.py` there. Keeping Arduino App Lab open (or running the official platform-tools ADB server) provides the laptop's local ADB server on port 5037. Bob talks to that server directly; it does not need to ship an `adb.exe` binary.

The existing COM-port connection remains for ESP32 testing. Its old handshake sketch does not implement tasks or screen frames, so Bob will connect but keep task submission disabled until a compatible service is present.

## What this service does

- Replies to the versioned hello and heartbeat.
- Advertises `tasks_v1` and `screen_v1` capabilities, and explicitly reports `ai_ready: false`.
- Receives a task with a unique ID, selected mode (`do` or `guide`), and text; replies `task_received` with the same ID.
- Receives small JPEG screen frames in ordered base64 chunks, checks total length and SHA-256, and acknowledges each completed frame.
- Keeps only the most recent frame in RAM. Screens are not written to disk.
- Drops task and frame references on Stop, a closed connection, an idle host heartbeat, or an eight-second session lease expiry.
- Sends metadata logs to Bob's Debug view. Task text and image payloads are not logged.

It does **not** run an AI model, produce guidance/pointers, or issue mouse or keyboard actions. Both modes are carried through the protocol so the model/action layer can be added next.

## Wire protocol

UTF-8 JSON, one object per newline. Messages use `protocol: 1` and a request `nonce`; replies echo that nonce and include `device: "device-companion"`. Task messages also echo `task_id`; frame replies echo `frame_id`. Limits: 8,192-byte lines, 4,000-byte task text, 32,768-byte frames, 768 decoded bytes per chunk.

| Host request | Device response |
|---|---|
| `hello` | `hello_ack` with board, capabilities and AI status |
| `ping` with active `task_id`, or null when idle | `pong` |
| `task` with `task_id`, `mode`, `text` | `task_received` |
| `screen_begin` with `task_id`, `frame_id`, `size` | `screen_ready` |
| `screen_chunk` with IDs, zero-based `index`, base64 `data` | No response per chunk |
| `screen_end` with IDs and `sha256` | `screen_received` |
| `stop_task` with `task_id` | `task_stopped` |

The receiver may send `log` messages before a reply, or an `error` with the request nonce. Stop is idempotent. Duplicate task requests with the same ID return the original receipt rather than starting another task. Late screen messages for stopped tasks are rejected.

This is a compatibility check, not cryptographic device authentication. The task layer has no shell or input-execution command.

## Test without hardware

```powershell
python -m unittest discover -s uno-q -p "test_*.py" -v
```

The tests use synthetic image bytes and loopback sockets. They do not capture your screen or require a connected board.

References: [Arduino Debian/ADB guide](https://docs.arduino.cc/tutorials/uno-q/debian-guide/), [ADB smart-socket services](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/docs/dev/services.md).

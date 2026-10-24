#!/usr/bin/env python3
"""Bob's UNO Q receiver. Standard library only; no AI and no desktop actions."""
import argparse
import base64
import hashlib
import json
import socket
import time

MAX_LINE = 8192
MAX_FRAME = 32768
LEASE_SECONDS = 8


class Receiver:
    def __init__(self):
        self.task = None
        self.frame = None
        self.last_frame = None  # One JPEG in RAM; never written to disk.
        self.last_seen = 0

    def stop(self):
        self.task = self.frame = self.last_frame = None

    def expire(self):
        if self.task and time.monotonic() - self.last_seen > LEASE_SECONDS:
            self.stop()
            return True
        return False

    def handle(self, packet):
        if not isinstance(packet, dict) or packet.get('protocol') != 1:
            return []
        kind, nonce = packet.get('type'), packet.get('nonce')
        if not isinstance(nonce, str) or len(nonce) > 100:
            return []
        def reply(type_, **extra):
            return dict(type=type_, protocol=1, device='device-companion', nonce=nonce, **extra)
        def log(message):
            return dict(type='log', protocol=1, device='device-companion', message=message, level='info')
        self.expire()
        self.last_seen = time.monotonic()
        if kind == 'hello':
            return [reply('hello_ack', board='Arduino UNO Q', service_version='0.3.0',
                          capabilities=['tasks_v1', 'screen_v1'], ai_ready=False)]
        if kind == 'ping':
            if self.task and packet.get('task_id') != self.task['id']:
                self.stop()
                return [log('Host session is inactive. Screen buffers cleared.'), reply('pong')]
            return [reply('pong')]
        if kind == 'task':
            task_id, mode, text = packet.get('task_id'), packet.get('mode'), packet.get('text')
            if (not isinstance(task_id, str) or not 1 <= len(task_id) <= 100
                    or mode not in ('do', 'guide') or not isinstance(text, str)
                    or not text.strip() or len(text.encode('utf-8')) > 4000):
                return [reply('error', message='Invalid task or mode.')]
            if self.task and self.task['id'] == task_id:
                return [reply('task_received', task_id=task_id, mode=mode, ai_ready=False)]
            if self.task:
                return [reply('error', message='Stop the current task before starting another.')]
            self.task = dict(id=task_id, mode=mode, text=text)
            return [log(f'Task received ({mode} mode). AI is not installed.'),
                    reply('task_received', task_id=task_id, mode=mode, ai_ready=False)]
        if kind == 'stop_task':
            task_id = packet.get('task_id')
            if self.task and task_id != self.task['id']:
                return [reply('error', message='Task ID does not match.')]
            self.stop()
            return [log('Session stopped. Screen buffers cleared.'), reply('task_stopped', task_id=task_id)]
        if not self.task or packet.get('task_id') != self.task['id']:
            return [reply('error', message='No matching active task.')]
        if kind == 'screen_begin':
            size, frame_id = packet.get('size'), packet.get('frame_id')
            if not isinstance(size, int) or not 1 <= size <= MAX_FRAME or not isinstance(frame_id, str) or len(frame_id) > 100:
                return [reply('error', message='Invalid frame size or ID.')]
            self.frame = dict(id=frame_id, size=size, data=bytearray(), next=0)
            return [reply('screen_ready', frame_id=frame_id, task_id=self.task['id'])]
        if kind == 'screen_chunk':
            frame = self.frame
            if not frame or packet.get('frame_id') != frame['id'] or packet.get('index') != frame['next']:
                self.frame = None
                return [reply('error', message='Out-of-order screen chunk.')]
            try:
                payload = packet.get('data')
                if not isinstance(payload, str) or len(payload) > 1024:
                    raise ValueError('Oversized chunk')
                data = base64.b64decode(payload, validate=True)
            except (ValueError, TypeError):
                self.frame = None
                return [reply('error', message='Invalid screen chunk.')]
            if len(frame['data']) + len(data) > frame['size']:
                self.frame = None
                return [reply('error', message='Screen frame exceeded declared size.')]
            frame['data'].extend(data)
            frame['next'] += 1
            return []
        if kind == 'screen_end':
            frame = self.frame
            self.frame = None
            if (not frame or packet.get('frame_id') != frame['id']
                    or len(frame['data']) != frame['size']
                    or hashlib.sha256(frame['data']).hexdigest() != packet.get('sha256')):
                return [reply('error', message='Screen frame failed validation.')]
            self.last_frame = bytes(frame['data'])
            return [log(f'Screen frame received: {len(self.last_frame)} bytes (RAM only).'),
                    reply('screen_received', task_id=self.task['id'], frame_id=frame['id'])]
        return [reply('error', message='Unsupported message type.')]


def serve_connection(connection):
    receiver = Receiver()
    connection.settimeout(0.5)
    pending = bytearray()
    try:
        while True:
            if receiver.expire():
                print('Session lease expired; screen buffers cleared.', flush=True)
            try:
                data = connection.recv(4096)
            except socket.timeout:
                continue
            if not data:
                break
            pending.extend(data)
            while b'\n' in pending:
                line, _, pending = pending.partition(b'\n')
                if len(line) > MAX_LINE:
                    raise ValueError('Message too large')
                try:
                    packet = json.loads(line)
                except (ValueError, UnicodeError):
                    continue
                for response in receiver.handle(packet):
                    connection.sendall((json.dumps(response, separators=(',', ':')) + '\n').encode())
            if len(pending) > MAX_LINE:
                raise ValueError('Message too large')
    except (OSError, ValueError) as exc:
        print(f'Connection ended: {exc}', flush=True)
    finally:
        receiver.stop()
        connection.close()
        print('USB session closed; screen buffers cleared.', flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=8765)
    args = parser.parse_args()
    # Loopback only. The laptop reaches this socket through the USB ADB transport.
    with socket.socket() as server:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind(('127.0.0.1', args.port))
        server.listen(1)
        print(f'Bob receiver listening on 127.0.0.1:{args.port}; awaiting USB ADB connection.', flush=True)
        while True:
            connection, _ = server.accept()
            serve_connection(connection)  # One controller at a time.


if __name__ == '__main__':
    main()

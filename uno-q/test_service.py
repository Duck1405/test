import base64
import hashlib
import json
import socket
import threading
import time
import unittest
from bob_service import Receiver, serve_connection


class ReceiverTests(unittest.TestCase):
    def setUp(self):
        self.receiver = Receiver()

    def send(self, kind, **kwargs):
        return self.receiver.handle(dict(type=kind, protocol=1, nonce='test', **kwargs))

    def task(self):
        return self.send('task', task_id='task-1', mode='guide', text='Open a document')

    def test_hello_declares_capabilities_without_claiming_ai(self):
        response = self.send('hello')[0]
        self.assertEqual(response['type'], 'hello_ack')
        self.assertEqual(response['nonce'], 'test')
        self.assertIn('screen_v1', response['capabilities'])
        self.assertFalse(response['ai_ready'])

    def test_task_receipt_is_idempotent_and_validates_mode(self):
        self.assertEqual(self.task()[-1]['type'], 'task_received')
        self.assertEqual(self.task()[-1]['type'], 'task_received')
        self.assertEqual(self.send('task', task_id='task-2', mode='guide', text='Other')[-1]['type'], 'error')
        self.send('stop_task', task_id='task-1')
        self.assertEqual(self.send('task', task_id='bad', mode='shell', text='Other')[-1]['type'], 'error')
        self.assertIsNone(self.receiver.task)

    def test_screen_chunks_validate_hash_and_stop_clears_memory(self):
        self.task()
        data = b'\xff\xd8fake-jpeg-for-transport-test\xff\xd9'
        self.assertEqual(self.send('screen_begin', task_id='task-1', frame_id='frame-1', size=len(data))[-1]['type'], 'screen_ready')
        for i, offset in enumerate(range(0, len(data), 8)):
            self.assertEqual(self.send('screen_chunk', task_id='task-1', frame_id='frame-1', index=i, data=base64.b64encode(data[offset:offset + 8]).decode()), [])
        response = self.send('screen_end', task_id='task-1', frame_id='frame-1', sha256=hashlib.sha256(data).hexdigest())
        self.assertEqual(response[-1]['type'], 'screen_received')
        self.assertEqual(self.receiver.last_frame, data)
        self.assertEqual(self.send('stop_task', task_id='task-1')[-1]['type'], 'task_stopped')
        self.assertIsNone(self.receiver.last_frame)
        self.assertIsNone(self.receiver.frame)
        self.assertIsNone(self.receiver.task)
        self.assertEqual(self.send('screen_begin', task_id='task-1', frame_id='late', size=10)[-1]['type'], 'error')

    def test_bad_chunks_and_oversized_frames_are_rejected(self):
        self.task()
        self.assertEqual(self.send('screen_begin', task_id='task-1', frame_id='bad', size=999999)[-1]['type'], 'error')
        self.send('screen_begin', task_id='task-1', frame_id='good', size=10)
        self.assertEqual(self.send('screen_chunk', task_id='task-1', frame_id='good', index=1, data='YWJj')[-1]['type'], 'error')
        self.assertIsNone(self.receiver.frame)

    def test_lease_expires_and_idle_heartbeat_cancels_task(self):
        self.task()
        self.receiver.last_seen = time.monotonic() - 9
        self.assertTrue(self.receiver.expire())
        self.assertIsNone(self.receiver.task)
        self.task()
        self.send('ping', task_id='task-1')
        self.assertIsNotNone(self.receiver.task)
        self.send('ping')
        self.assertIsNone(self.receiver.task)

    def test_socket_session_handles_fragmented_json(self):
        host, board = socket.socketpair()
        host.settimeout(2)
        worker = threading.Thread(target=serve_connection, args=(board,), daemon=True)
        worker.start()
        try:
            host.sendall(b'{"type":"he')
            host.sendall(b'llo","protocol":1,"nonce":"socket-test"}\n')
            pending = bytearray()
            while b'\n' not in pending:
                pending.extend(host.recv(1024))
            self.assertEqual(json.loads(pending)['nonce'], 'socket-test')
        finally:
            host.close()
            worker.join(2)
        self.assertFalse(worker.is_alive())


if __name__ == '__main__':
    unittest.main()

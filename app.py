"""Windows-first serial companion. No desktop automation is enabled yet."""
import queue
import threading
import tkinter as tk
from datetime import datetime
from tkinter import ttk
from tkinter.scrolledtext import ScrolledText

import serial
from serial.tools import list_ports


class Companion:
    def __init__(self, root):
        self.root = root
        self.events = queue.Queue(maxsize=512)
        self.stop = threading.Event()
        self.worker = None
        self.connected = False
        self.closing = False
        self.ports = {}
        root.title('Device Companion')
        root.geometry('720x500')
        root.minsize(600, 400)
        root.configure(bg='#f4f6fa')
        style = ttk.Style()
        style.theme_use('clam')
        style.configure('TFrame', background='#f4f6fa')
        style.configure('TLabel', background='#f4f6fa', font=('Segoe UI', 10))
        style.configure('Title.TLabel', font=('Segoe UI', 23, 'bold'))
        style.configure('TButton', font=('Segoe UI', 10), padding=8)
        panel = ttk.Frame(root, padding=24)
        panel.pack(fill='both', expand=True)
        ttk.Label(panel, text='Device Companion', style='Title.TLabel').pack(anchor='w')
        ttk.Label(panel, text='Connect your AI device over USB serial.').pack(anchor='w', pady=(4, 22))
        row = ttk.Frame(panel)
        row.pack(fill='x')
        ttk.Label(row, text='COM port').grid(row=0, column=0, sticky='w')
        ttk.Label(row, text='Baud rate').grid(row=0, column=1, sticky='w', padx=12)
        self.port = ttk.Combobox(row, state='readonly', width=42)
        self.port.grid(row=1, column=0, sticky='ew', pady=6)
        self.baud = ttk.Combobox(row, state='readonly', values=['9600', '57600', '115200', '230400', '460800', '921600'], width=10)
        self.baud.set('115200')
        self.baud.grid(row=1, column=1, padx=12)
        self.refresh_button = ttk.Button(row, text='Refresh', command=self.refresh)
        self.refresh_button.grid(row=1, column=2)
        row.columnconfigure(0, weight=1)
        controls = ttk.Frame(panel)
        controls.pack(fill='x', pady=(12, 16))
        self.button = ttk.Button(controls, text='Connect', command=self.toggle)
        self.button.pack(side='left')
        self.status = ttk.Label(controls, text='Disconnected')
        self.status.pack(side='left', padx=16)
        ttk.Label(panel, text='Connection log').pack(anchor='w')
        self.log = ScrolledText(panel, height=10, state='disabled', font=('Consolas', 10), bg='#152033', fg='#e2e8f0', relief='flat', padx=12, pady=12)
        self.log.pack(fill='both', expand=True, pady=(8, 10))
        ttk.Label(panel, text='Serial connection only • AI and desktop control are not enabled yet.').pack(anchor='w')
        root.protocol('WM_DELETE_WINDOW', self.close)
        self.refresh()
        root.after(75, self.poll)

    def append(self, message):
        self.log.configure(state='normal')
        self.log.insert('end', f'{datetime.now():%H:%M:%S}  {message}\n')
        if int(self.log.index('end-1c').split('.')[0]) > 600:
            self.log.delete('1.0', '100.0')
        self.log.see('end')
        self.log.configure(state='disabled')

    def refresh(self):
        previous = self.port.get()
        try:
            self.ports = {f'{p.device} — {p.description}': p.device for p in sorted(list_ports.comports())}
        except Exception as exc:
            self.append(f'Port scan failed: {exc}')
            return
        self.port['values'] = list(self.ports)
        self.port.set(previous if previous in self.ports else next(iter(self.ports), ''))
        self.button.configure(state='normal' if self.ports else 'disabled')
        self.status.configure(text='Disconnected' if self.ports else 'No COM ports found — plug in your device and refresh')

    def emit(self, kind, value=''):
        try:
            self.events.put_nowait((kind, value))
        except queue.Full:
            if kind != 'data':
                self.events.put((kind, value))

    def toggle(self):
        if self.worker and self.worker.is_alive():
            self.stop.set()
            self.button.configure(state='disabled')
            self.status.configure(text='Disconnecting…')
            return
        device = self.ports.get(self.port.get())
        if not device:
            return
        self.stop.clear()
        self.button.configure(state='disabled')
        self.port.configure(state='disabled')
        self.baud.configure(state='disabled')
        self.refresh_button.configure(state='disabled')
        self.status.configure(text=f'Connecting to {device}…')
        self.worker = threading.Thread(target=self.serial_loop, args=(device, int(self.baud.get())), daemon=True)
        self.worker.start()

    def serial_loop(self, device, baud):
        try:
            # Configure control lines before opening to reduce board resets.
            with serial.Serial(port=None, baudrate=baud, timeout=0.2, write_timeout=1) as connection:
                connection.dtr = False
                connection.rts = False
                connection.port = device
                connection.open()
                self.emit('connected', f'{device} at {baud} baud')
                while not self.stop.is_set():
                    data = connection.read(512)
                    if data:
                        self.emit('data', data.decode('utf-8', errors='replace').replace('\x00', ''))
        except (serial.SerialException, OSError) as exc:
            self.emit('error', str(exc))
        finally:
            self.emit('disconnected')

    def poll(self):
        for _ in range(100):
            try:
                kind, value = self.events.get_nowait()
            except queue.Empty:
                break
            if kind == 'connected':
                self.connected = True
                self.status.configure(text=f'Serial connected: {value}')
                self.button.configure(text='Disconnect', state='normal')
                self.append(f'Opened {value}. Device identity is not verified.')
            elif kind == 'data':
                self.append(f'RX: {value.rstrip()}')
            elif kind == 'error':
                self.append(f'Connection error: {value}')
            elif kind == 'disconnected':
                self.connected = False
                self.button.configure(text='Connect', state='normal' if self.ports else 'disabled')
                self.port.configure(state='readonly')
                self.baud.configure(state='readonly')
                self.refresh_button.configure(state='normal')
                self.status.configure(text='Disconnected')
                self.append('Serial connection closed.')
        if self.closing and (not self.worker or not self.worker.is_alive()):
            self.root.destroy()
            return
        self.root.after(75, self.poll)

    def close(self):
        self.closing = True
        self.stop.set()
        self.root.withdraw()


if __name__ == '__main__':
    root = tk.Tk()
    Companion(root)
    root.mainloop()

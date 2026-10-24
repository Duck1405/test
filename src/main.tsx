import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './style.css';

type Port = { name: string; label: string; usb: boolean };
type DeviceState = { phase: 'disconnected' | 'connecting' | 'connected' | 'disconnecting'; port: string; error: string };
const initial: DeviceState = { phase: 'disconnected', port: '', error: '' };

function DeviceIcon({ connected }: { connected: boolean }) {
  return <div className={`device-icon ${connected ? 'ready' : ''}`} aria-hidden="true">
    <svg viewBox="0 0 56 56" fill="none">
      <rect x="17" y="10" width="23" height="33" rx="6" stroke="currentColor" strokeWidth="1.8" />
      <path d="M24 16h9M28.5 43v5m-4 0h8" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      <circle cx="28.5" cy="36" r="1.5" fill="currentColor" />
      <circle cx="41" cy="41" r="10" className="icon-badge" stroke="white" strokeWidth="2.5" />
      {connected ? <path d="m37 41 3 3 5-6" stroke="white" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
        : <path d="m38 38 6 6m0-6-6 6" stroke="white" strokeWidth="1.8" strokeLinecap="round" />}
    </svg>
  </div>;
}

function App() {
  const [device, setDevice] = useState<DeviceState>(initial);
  const [ports, setPorts] = useState<Port[]>([]);
  const [selected, setSelected] = useState('');
  const [uiError, setUiError] = useState('');
  const [refreshKey, setRefreshKey] = useState(0);
  const attempted = useRef(new Set<string>());
  const connecting = useRef(false);
  const manual = useRef(false);
  const current = useRef(device);
  const native = isTauri();
  const connected = device.phase === 'connected';
  const busy = device.phase === 'connecting' || device.phase === 'disconnecting';

  async function connect(port: string, automatic = false) {
    if (!native || !port || connecting.current || current.current.phase !== 'disconnected') return;
    connecting.current = true;
    manual.current = !automatic;
    attempted.current.add(port);
    setUiError('');
    try {
      await invoke('connect_device', { port });
      const next = await invoke<DeviceState>('get_state');
      current.current = next;
      setDevice(next);
    } catch (error) {
      setUiError(String(error));
    } finally {
      connecting.current = false;
    }
  }

  useEffect(() => {
    if (!native) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await invoke<DeviceState>('get_state');
        if (active) { current.current = next; setDevice(next); }
      } catch (error) { if (active) setUiError(String(error)); }
      if (active) timer = setTimeout(poll, 350);
    }
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [native]);

  useEffect(() => {
    if (!native) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function scan() {
      try {
        const found = await invoke<Port[]>('list_ports');
        if (!active) return;
        setPorts(found);
        setSelected(previous => found.some(p => p.name === previous) ? previous : found[0]?.name ?? '');
        attempted.current.forEach(p => { if (!found.some(item => item.name === p)) attempted.current.delete(p); });
        if (!manual.current && current.current.phase === 'disconnected' && !connecting.current) {
          const candidate = found.find(p => p.usb && !attempted.current.has(p.name));
          if (candidate) { setSelected(candidate.name); await connect(candidate.name, true); }
        }
      } catch (error) { if (active) setUiError(String(error)); }
      if (active) timer = setTimeout(scan, 2000);
    }
    void scan();
    return () => { active = false; clearTimeout(timer); };
  }, [native, refreshKey]);

  async function disconnect() {
    manual.current = true;
    try {
      await invoke('disconnect_device');
      const next = await invoke<DeviceState>('get_state');
      current.current = next; setDevice(next);
    } catch (error) { setUiError(String(error)); }
  }

  const error = uiError || device.error;
  const status = busy ? (device.phase === 'connecting' ? `Checking ${device.port}…` : 'Disconnecting…')
    : connected ? `${device.port} · USB connected` : error ? 'Let’s try again.'
      : ports.length ? 'Choose a port, or let Bob find it.' : 'Waiting for your USB device…';

  return <main className="widget">
    <header onPointerDown={event => {
      if (native && event.button === 0 && !(event.target as HTMLElement).closest('button'))
        void getCurrentWindow().startDragging().catch(error => setUiError(String(error)));
    }}>
      <div className="brand"><span className="bowtie" aria-hidden="true">◆</span> Bob the Butler</div>
      <div className="header-actions"><span className={`pill ${connected ? 'online' : ''}`}><i />{connected ? 'Connected' : 'Offline'}</span>
        <button className="close" aria-label="Close Bob the Butler" onClick={() => {
          if (native) void getCurrentWindow().close().catch(error => setUiError(String(error)));
        }}>×</button>
      </div>
    </header>
    <section className="hero">
      <DeviceIcon connected={connected} />
      <div><h1>{connected ? 'At your service.' : 'Hey, let’s connect.'}</h1>
        <p>{connected ? 'Your device is verified and connected.' : 'Plug in your device to get started.'}</p></div>
    </section>
    <div className="status" role="status" aria-live="polite"><span className={busy ? 'pulse' : ''} />{status}</div>
    {connected ? <div className="connected-content"><p>Task controls and local AI are coming next.</p>
      <button className="disconnect" onClick={() => void disconnect()}>Disconnect device</button></div>
      : <><div className="port-row"><label className="sr-only" htmlFor="port">COM port</label>
        <select id="port" value={selected} disabled={busy || !ports.length} onChange={e => setSelected(e.target.value)}>
          {!ports.length && <option value="">{native ? 'No COM ports found' : 'Desktop app required'}</option>}
          {ports.map(port => <option value={port.name} key={port.name}>{port.label}</option>)}
        </select>
        <button className="connect" disabled={busy || !selected || !native} aria-label="Connect to selected COM port" title="Check and connect" onClick={() => void connect(selected)}>
          {busy ? <span className="spinner" /> : <span aria-hidden="true">→</span>}
        </button></div>
        <button className="refresh" disabled={busy || !native} onClick={() => {
          manual.current = false; attempted.current.clear(); setUiError(''); setRefreshKey(v => v + 1);
        }}><span aria-hidden="true">↻</span> Refresh ports</button></>}
    <div className="feedback" role="alert">{error || (!native ? 'Browser preview only. Open the Windows app to connect.' : '')}</div>
    <footer><span className="lock" aria-hidden="true">◇</span> Your device. Your control.</footer>
  </main>;
}

createRoot(document.getElementById('root')!).render(<App />);

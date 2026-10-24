import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './style.css';

type Port = { name: string; label: string; usb: boolean };
type DeviceState = {
  phase: 'disconnected' | 'connecting' | 'connected' | 'disconnecting'; port: string; error: string;
  capabilities: string[]; task_phase: 'idle' | 'sending' | 'active' | 'stopping' | 'stopped';
  task_id: string; mode: string; message: string; frames: number;
};
type LogEntry = { time: number; source: 'laptop' | 'uno'; message: string };
type Chat = { role: 'user' | 'bob'; text: string };
const initial: DeviceState = { phase: 'disconnected', port: '', error: '', capabilities: [], task_phase: 'idle', task_id: '', mode: 'guide', message: '', frames: 0 };

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

function SharingOverlay() {
  return <div className="screen-border" aria-label="Primary screen is being shared with UNO Q">
    <span className="corner tl" /><span className="corner tr" /><span className="corner bl" /><span className="corner br" />
  </div>;
}
function StopPopup() {
  const [error, setError] = useState('');
  return <div className="stop-popup"><span className="live-dot" /><span>{error || 'Screen shared with UNO Q'}</span>
    <button onClick={() => void invoke('stop_task').catch(e => setError(String(e)))}>Stop <kbd>Esc</kbd></button></div>;
}

function App() {
  const [device, setDevice] = useState<DeviceState>(initial);
  const [ports, setPorts] = useState<Port[]>([]);
  const [selected, setSelected] = useState('');
  const [uiError, setUiError] = useState('');
  const [refreshKey, setRefreshKey] = useState(0);
  const [debug, setDebug] = useState(false);
  const [source, setSource] = useState('all');
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [task, setTask] = useState('');
  const [mode, setMode] = useState('guide');
  const [chat, setChat] = useState<Chat[]>([]);
  const attempted = useRef(new Set<string>());
  const connecting = useRef(false);
  const manual = useRef(false);
  const current = useRef(device);
  const lastMessage = useRef('');
  const chatBottom = useRef<HTMLDivElement>(null);
  const native = isTauri();
  const connected = device.phase === 'connected';
  const busy = device.phase === 'connecting' || device.phase === 'disconnecting';
  const sessionBusy = ['sending', 'active', 'stopping'].includes(device.task_phase);
  const supported = device.capabilities.includes('tasks_v1') && device.capabilities.includes('screen_v1');

  async function sync() {
    const next = await invoke<DeviceState>('get_state');
    current.current = next; setDevice(next);
  }
  async function connect(port: string, automatic = false) {
    if (!native || !port || connecting.current || current.current.phase !== 'disconnected') return;
    connecting.current = true; manual.current = !automatic; attempted.current.add(port); setUiError('');
    try { await invoke('connect_device', { port }); await sync(); }
    catch (error) { setUiError(String(error)); }
    finally { connecting.current = false; }
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
      if (active) timer = setTimeout(poll, 250);
    }
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [native]);
  useEffect(() => {
    if (!native || !debug) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try { const next = await invoke<LogEntry[]>('get_logs'); if (active) setLogs(next); }
      catch (error) { if (active) setUiError(String(error)); }
      if (active) timer = setTimeout(poll, 500);
    }
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, [native, debug]);
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
  useEffect(() => {
    const key = `${device.task_id}:${device.message}`;
    if (device.message && device.task_phase !== 'sending' && key !== lastMessage.current) {
      lastMessage.current = key;
      setChat(previous => [...previous, { role: 'bob' as const, text: device.message }].slice(-30));
    }
  }, [device.task_id, device.task_phase, device.message]);
  useEffect(() => { chatBottom.current?.scrollIntoView({ block: 'end' }); }, [chat]);
  useEffect(() => {
    if (!native) return;
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') void invoke('stop_task').catch(e => setUiError(String(e))); };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [native]);

  async function disconnect() {
    manual.current = true;
    try { await invoke('disconnect_device'); await sync(); }
    catch (error) { setUiError(String(error)); }
  }
  async function sendTask(event: React.FormEvent) {
    event.preventDefault();
    const text = task.trim();
    if (!text || sessionBusy) return;
    setUiError('');
    try {
      await invoke('start_task', { text, mode });
      setChat(previous => [...previous, { role: 'user' as const, text }].slice(-30));
      setTask(''); await sync();
    } catch (error) { setUiError(String(error)); }
  }
  async function stopTask() {
    try { await invoke('stop_task'); await sync(); }
    catch (error) { setUiError(String(error)); }
  }
  const error = uiError || device.error;
  const status = busy ? (device.phase === 'connecting' ? `Checking ${device.port.startsWith('adb:') ? 'UNO Q USB' : device.port}…` : 'Disconnecting…')
    : connected ? (device.task_phase === 'active' ? `Sharing primary screen · ${device.frames} frames received` : device.task_phase === 'sending' ? 'Waiting for UNO Q receipt…' : device.task_phase === 'stopping' ? 'Sharing off · finishing stop…' : 'UNO Q connected · screen sharing off')
      : error ? 'Let’s try again.' : ports.length ? 'Choose a device, or let Bob find it.' : 'Waiting for your USB device…';

  return <main className={`widget ${connected ? 'has-chat' : ''}`}>
    <header onPointerDown={event => {
      if (native && event.button === 0 && !(event.target as HTMLElement).closest('button'))
        void getCurrentWindow().startDragging().catch(e => setUiError(String(e)));
    }}>
      <div className="brand"><span className="bowtie" aria-hidden="true">◆</span> Bob the Butler</div>
      <div className="header-actions"><span className={`pill ${connected ? 'online' : ''}`}><i />{connected ? 'Online' : 'Offline'}</span>
        <button className="close" aria-label="Close Bob the Butler" onClick={() => {
          if (native) void getCurrentWindow().close().catch(e => setUiError(String(e)));
        }}>×</button>
      </div>
    </header>
    {debug ? <section className="debug-view">
      <div className="debug-heading"><button onClick={() => setDebug(false)}>← Back</button><strong>Debug logs</strong>
        <button disabled={!native} onClick={() => void invoke('diagnose_usb').catch(e => setUiError(String(e)))}>Check USB</button></div>
      <div className="log-tabs">{[['all', 'All'], ['laptop', 'Laptop'], ['uno', 'UNO Q']].map(([value, label]) => <button key={value} className={source === value ? 'selected' : ''} onClick={() => setSource(value)}>{label}</button>)}</div>
      <div className="logs" role="log" aria-label="Device and laptop debug logs">
        {logs.filter(log => source === 'all' || log.source === source).map((log, i) => <div className={`log-line ${log.source}`} key={`${log.time}:${i}`}><time>{new Date(log.time).toLocaleTimeString()}</time><b>{log.source === 'uno' ? 'Q' : 'PC'}</b><span>{log.message}</span></div>)}
        {!logs.some(log => source === 'all' || log.source === source) && <p>{source === 'uno' ? 'UNO Q logs will appear after its service connects.' : 'No logs yet.'}</p>}
      </div>
      <p className="log-note">Metadata only. Screens and task text are not logged.</p>
    </section> : connected ? <>
      <div className={`status chat-status ${device.task_phase === 'active' ? 'sharing' : ''}`} role="status"><span className={device.task_phase === 'sending' ? 'pulse' : ''} />{status}
        {sessionBusy ? <button onClick={() => void stopTask()}>Stop</button> : <button onClick={() => void disconnect()}>Disconnect</button>}
      </div>
      <div className="chat-messages" role="log" aria-label="Conversation with Bob">
        {!chat.length && <div className="bubble bob">{supported ? 'Ready when you are. Send a task to start sharing your primary screen with UNO Q. AI is coming next.' : 'Connected. Upload the Bob receiver service to UNO Q to enable tasks and screen sharing.'}</div>}
        {chat.map((message, i) => <div className={`bubble ${message.role}`} key={i}>{message.text}</div>)}
        <div ref={chatBottom} />
      </div>
      <form className="composer" onSubmit={event => void sendTask(event)}>
        <textarea aria-label="Task for UNO Q" placeholder="What can Bob help with?" value={task} maxLength={2000} disabled={sessionBusy || !supported} onChange={e => setTask(e.target.value)} />
        <div className="composer-controls"><select aria-label="Assistant mode" value={mode} disabled={sessionBusy} onChange={e => setMode(e.target.value)}><option value="do">Do it for me</option><option value="guide">Guide me</option></select>
          <button className="send" type="submit" disabled={!task.trim() || sessionBusy || !supported} title="Send task and share primary screen" aria-label="Send task and share primary screen">↑</button></div>
      </form>
      <p className="consent-note">Send shares your primary screen with UNO Q. <kbd>Esc</kbd> stops.</p>
    </> : <>
      <section className="hero"><DeviceIcon connected={false} /><div><h1>Hey, let’s connect.</h1><p>Plug in your UNO Q to get started.</p></div></section>
      <div className="status" role="status" aria-live="polite"><span className={busy ? 'pulse' : ''} />{status}</div>
      <div className="port-row"><label className="sr-only" htmlFor="port">USB device or COM port</label>
        <select id="port" value={selected} disabled={busy || !ports.length} onChange={e => setSelected(e.target.value)}>
          {!ports.length && <option value="">{native ? 'No USB devices or COM ports' : 'Desktop app required'}</option>}
          {ports.map(port => <option value={port.name} key={port.name}>{port.label}</option>)}
        </select>
        <button className="connect" disabled={busy || !selected || !native} aria-label="Connect to selected device" title="Check and connect" onClick={() => void connect(selected)}>{busy ? <span className="spinner" /> : <span aria-hidden="true">→</span>}</button>
      </div>
      <button className="refresh" disabled={busy || !native} onClick={() => { manual.current = false; attempted.current.clear(); setUiError(''); setRefreshKey(v => v + 1); }}><span aria-hidden="true">↻</span> Refresh devices</button>
    </>}
    {!!error && <div className={`feedback ${connected ? 'compact-error' : ''}`} role="alert">{error}</div>}
    {!native && !debug && <div className="feedback" role="alert">Browser preview only. Open the Windows app to connect.</div>}
    <footer><span>◇ Your device. Your control.</span><button onClick={() => setDebug(value => !value)}>{debug ? 'Close debug' : 'Debug'}</button></footer>
  </main>;
}

const view = new URLSearchParams(window.location.search).get('view');
createRoot(document.getElementById('root')!).render(view === 'overlay' ? <SharingOverlay /> : view === 'stop' ? <StopPopup /> : <App />);

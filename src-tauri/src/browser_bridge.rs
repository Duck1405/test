//! Read-only page observations from Bob's separate Chrome profile.
//! Input remains native SendInput; no model-authored JavaScript is evaluated.
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

pub fn profile() -> Result<PathBuf, String> {
    let base =
        std::env::var_os("LOCALAPPDATA").ok_or("Local app data directory is unavailable.")?;
    Ok(PathBuf::from(base)
        .join("BobTheButler")
        .join("ChromeProfile"))
}

const OBSERVE: &str = r#"(() => {
 const primary = screen.availLeft === 0 && screen.availTop >= 0;
 const border = Math.max(0,(outerWidth-innerWidth)/2);
 function target(e,kind,label,id='') {
  if(!e || !primary) return null;
  const r=e.getBoundingClientRect(),s=getComputedStyle(e);
  if(r.width<4||r.height<4||s.visibility!=='visible'||s.display==='none'||r.bottom<=0||r.top>=innerHeight||r.right<=0||r.left>=innerWidth) return null;
  const cx=(Math.max(0,r.left)+Math.min(innerWidth,r.right))/2;
  const cy=(Math.max(0,r.top)+Math.min(innerHeight,r.bottom))/2;
  const top=document.elementFromPoint(cx,cy);
  if(top && top!==e && !e.contains(top) && !top.contains(e)) return null;
  const x=(screenX+border+cx)/screen.width;
  const y=(screenY+outerHeight-innerHeight-border+cy)/screen.height;
  if(x<0||x>1||y<0||y>1) return null;
  let text=String(label).replace(/\s+/g,' ').trim().slice(0,90);
  while(new TextEncoder().encode(text).length>110) text=Array.from(text).slice(0,-1).join('');
  return {kind,label:text||'Target',id,x,y};
 }
 const youtube=location.hostname==='www.youtube.com'||location.hostname==='youtube.com';
 const targets=[];
 if(youtube) for(const e of document.querySelectorAll('a#video-title,a#video-title-link')) {
  if(e.closest('ytd-ad-slot-renderer,ytd-promoted-video-renderer')) continue;
  const u=new URL(e.href,location.href),id=u.searchParams.get('v');
  if(u.pathname!=='/watch'||!id||!/^[-_a-zA-Z0-9]{11}$/.test(id)) continue;
  const t=target(e,'video',e.textContent.trim()||e.title,id);
  if(t && !targets.some(x=>x.id===id)) targets.push(t);
  if(targets.length===3) break;
 }
 const video=youtube?document.querySelector('video'):null;
 let media=null;
 if(video) {
  media={id:new URL(location.href).searchParams.get('v')||'',paused:video.paused,ended:video.ended,time:video.currentTime,ready:video.readyState,ad:!!document.querySelector('.html5-video-player.ad-showing')};
  if(video.paused) {const t=target(document.querySelector('button.ytp-play-button'),'play','Play video',media.id);if(t)targets.push(t);}
 }
 if(location.hostname==='consent.youtube.com'||youtube) {
  for(const e of document.querySelectorAll('button')) {
   if(e.textContent.trim().toLowerCase()==='reject all') {const t=target(e,'consent','Reject optional cookies');if(t){targets.unshift(t);break;}}
  }
 }
 return {url:location.href.slice(0,350),page_title:document.title.slice(0,180),page_loading:document.readyState!=='complete',visible:document.visibilityState==='visible',focused:document.hasFocus(),targets,media};
})()"#;

fn pages(port: u16) -> Result<Vec<Value>, String> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(400),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    while data.len() < 256 * 1024 {
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n]);
        if let Some(end) = data.windows(4).position(|x| x == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&data[..end]);
            let length = header.lines().find_map(|line| {
                line.split_once(':').and_then(|(key, value)| {
                    if key.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
            });
            if length.is_some_and(|length| data.len() >= end + 4 + length) {
                break;
            }
        }
    }
    let split = data
        .windows(4)
        .position(|x| x == b"\r\n\r\n")
        .ok_or("Invalid Chrome observation response.")?
        + 4;
    serde_json::from_slice(&data[split..]).map_err(|e| e.to_string())
}

fn observe_page(url: &str) -> Result<Value, String> {
    let parsed = tauri::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "ws" || parsed.host_str() != Some("127.0.0.1") {
        return Err("Invalid local Chrome endpoint.".into());
    }
    let mut config = tungstenite::protocol::WebSocketConfig::default();
    config.max_message_size = Some(256 * 1024);
    config.max_frame_size = Some(256 * 1024);
    let stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", parsed.port().ok_or("Missing Chrome port.")?)
            .parse()
            .unwrap(),
        Duration::from_millis(400),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    let (mut socket, _) =
        tungstenite::client_with_config(url, stream, Some(config)).map_err(|e| e.to_string())?;
    socket.send(tungstenite::Message::Text(json!({"id":1,"method":"Runtime.evaluate","params":{"expression":OBSERVE,"returnByValue":true,"timeout":300}}).to_string().into())).map_err(|e|e.to_string())?;
    for _ in 0..8 {
        let message = socket.read().map_err(|e| e.to_string())?;
        if let tungstenite::Message::Text(text) = message {
            let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if value["id"] == 1 {
                let result = value["result"]["result"]["value"].clone();
                if result.is_object() {
                    return Ok(result);
                }
                return Err("Chrome page observation failed.".into());
            }
        }
    }
    Err("Chrome page observation timed out.".into())
}

pub fn observe() -> Result<Value, String> {
    let text = std::fs::read_to_string(profile()?.join("DevToolsActivePort"))
        .map_err(|e| e.to_string())?;
    let port: u16 = text
        .lines()
        .next()
        .ok_or("Chrome is starting.")?
        .parse()
        .map_err(|_| "Invalid Chrome port.")?;
    let pages = pages(port)?;
    for page in pages.iter().filter(|p| p["type"] == "page").take(3) {
        if let Some(url) = page["webSocketDebuggerUrl"].as_str() {
            if let Ok(value) = observe_page(url) {
                if value["visible"] == true && value["focused"] == true {
                    return Ok(value);
                }
            }
        }
    }
    Err("Bob's Chrome tab is not in the foreground.".into())
}

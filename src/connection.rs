// Соединение с телефоном: adb-туннель, поток H.264 -> openh264 -> RGBA,
// и фоновые потоки. Протокол v2.7 сверен с server.c / demuxer.c / DesktopConnection.java.
use crate::adb::{Adb, ServerParams};
use crate::control::ControlMsg;
use openh264::decoder::{Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;
use openh264::OpenH264API;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SCRCPY_VERSION: &str = "2.7";
pub const SCRCPY_VERSION_V3: &str = "3.3.4";
pub const DEVICE_SERVER_PATH: &str = "/data/local/tmp/scrcpy-server.jar";
const SOCKET_PREFIX: &str = "scrcpy_";
/// маркер прерванной попытки (начато новое подключение)
const ABORTED_MSG: &str = "__aborted__";

const DEVICE_NAME_LEN: usize = 64;

// флаги в старших битах PTS (пакетный режим send_frame_meta=true)
const PACKET_FLAG_CONFIG: u64 = 1 << 63;
const PACKET_HEADER: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Connecting,
    Streaming,
    Disconnected,
    Failed(String),
}

#[derive(Clone)]
pub struct Frame {
    pub rgba: Arc<Vec<u8>>,
    pub w: usize,
    pub h: usize,
    pub version: u64,
}

#[derive(Default)]
pub struct Stats {
    pub fps: u32,
    pub decode_ms: f32,
    pub count: u64,
    pub bitrate_kbps: u64,
}

/// Состояние, разделяемое с UI.
pub struct Shared {
    pub status: Mutex<Status>,
    pub device_name: Mutex<String>,
    pub video_size: Mutex<(u32, u32)>,
    pub frame: Mutex<Option<Frame>>,
    pub stats: Mutex<Stats>,
    pub server_log: Mutex<Vec<String>>,
    pub snack: Mutex<Option<(String, Instant)>>,
    /// счётчик попыток подключения: поток со старым номером прерывается
    pub conn_seq: std::sync::atomic::AtomicU64,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(Status::Idle),
            device_name: Mutex::new(String::new()),
            video_size: Mutex::new((0, 0)),
            frame: Mutex::new(None),
            stats: Mutex::new(Stats::default()),
            server_log: Mutex::new(Vec::new()),
            snack: Mutex::new(None),
            conn_seq: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn log(&self, line: String) {
        let mut log = self.server_log.lock().unwrap();
        if log.len() > 500 {
            log.drain(0..200);
        }
        log.push(line.clone());
        drop(log);
        // дублируем в файл в рабочей папке (для диагностики извне)
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("mirror_debug.log")
        {
            use std::io::Write as _;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{}] {}", ts, line);
        }
    }

    pub fn set_snack(&self, text: String) {
        *self.snack.lock().unwrap() = Some((text, Instant::now()));
    }
}

/// Активное соединение (потоки + канал управления).
pub struct Connection {
    pub stop: Arc<AtomicBool>,
    pub ctrl_tx: Sender<ControlMsg>,
    pub child: Arc<Mutex<Option<Child>>>,
    pub port: u16,
    adb: Adb,
    serial: String,
    scid_hex: String,
    thread: Option<JoinHandle<()>>,
}

impl Connection {
    /// Мягкая остановка: флаг + убийство adb-shell процесса + добить
    /// сервер на устройстве по scid (иначе остаются зомби-процессы).
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
        // сервер на устройстве может пережить kill adb-клиента —
        // добиваем по scid (строке из его командной строки)
        let pk = format!("pkill -f scid={} || true", self.scid_hex);
        let _ = self.adb.run(&["-s", &self.serial, "shell", pk.as_str()], 3000);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Debug)]
enum ReadErr {
    TimedOut,
    Eof,
    Io(std::io::Error),
}

impl std::fmt::Display for ReadErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadErr::TimedOut => write!(f, "таймаут чтения"),
            ReadErr::Eof => write!(f, "поток закрыт"),
            ReadErr::Io(e) => write!(f, "ошибка сети: {}", e),
        }
    }
}

impl std::error::Error for ReadErr {}

fn read_u32be(stream: &mut TcpStream) -> Result<u32, ReadErr> {
    let mut b = [0u8; 4];
    read_full(stream, &mut b)?;
    Ok(u32::from_be_bytes(b))
}


/// read_exact с пониманием таймаутов (экран телефона может «молчать»).
fn read_full(stream: &mut TcpStream, buf: &mut [u8]) -> Result<(), ReadErr> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return Err(ReadErr::Eof),
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                return Err(ReadErr::TimedOut)
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(ReadErr::Io(e)),
        }
    }
    Ok(())
}

/// Собрать буферы вывода сервера (логи app_process) в shared.server_log.
fn drain_server_output(shared: Arc<Shared>, mut out: ChildStdout, mut err: Option<ChildStderr>) {
    let mut handle = |tag: &str, stream: &mut dyn Read| {
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]);
                    for line in text.lines() {
                        if !line.trim().is_empty() {
                            shared.log(format!("[server] {}", line));
                        }
                    }
                }
            }
        }
        let _ = tag;
    };
    handle("out", &mut out);
    if let Some(e) = err.as_mut() {
        handle("err", e);
    }
}

/// Запустить всё соединение в фоновом потоке.
pub fn start(
    adb: Adb,
    serial: String,
    jar_path: PathBuf,
    params: ServerParams,
    shared: Arc<Shared>,
    ctx: egui::Context,
    seq: u64,
) -> Arc<Connection> {
    let stop = Arc::new(AtomicBool::new(false));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<ControlMsg>();
    let scid_hex = format!("{:08x}", params.scid);

    let stop2 = stop.clone();
    let child_slot2 = child_slot.clone();
    #[allow(unused_variables)]
    let scid2 = scid_hex.clone();
    let adb2 = adb.clone();
    let serial2 = serial.clone();
    let handle = std::thread::spawn(move || {
        let result = run_connection(&adb2, &serial2, &jar_path, &params, &shared, &ctx, &stop2, &child_slot2, rx, seq);
        match result {
            Ok(()) => {
                *shared.status.lock().unwrap() = Status::Disconnected;
            }
            Err(e) => {
                if e.to_string() == ABORTED_MSG {
                    // попытку сменила более новая — статус не трогаем
                    shared.log("[conn] попытка отменена (начато новое подключение)".into());
                } else {
                    shared.log(format!("[conn] ошибка: {:#}", e));
                    *shared.status.lock().unwrap() = Status::Failed(format!("{:#}", e));
                }
            }
        }
        let _ = ctx.request_repaint();
    });

    Arc::new(Connection {
        stop,
        ctrl_tx: tx,
        child: child_slot,
        port: 0,
        adb,
        serial,
        scid_hex,
        thread: Some(handle),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_connection(
    adb: &Adb,
    serial: &str,
    jar_path: &PathBuf,
    params: &ServerParams,
    shared: &Arc<Shared>,
    ctx: &egui::Context,
    stop: &Arc<AtomicBool>,
    child_slot: &Arc<Mutex<Option<Child>>>,
    rx: mpsc::Receiver<ControlMsg>,
    seq: u64,
) -> anyhow::Result<()> {
    // попытка отменена, если пользователь начал новую
    let superseded = || shared.conn_seq.load(Ordering::Relaxed) != seq;
    // наш порт forward (для точечной уборки)
    let mut my_port: u16 = 0;

    // уборка: дочерний процесс + сервер на устройстве (по scid) + СВОЙ forward
    let cleanup = |child_slot: &Arc<Mutex<Option<Child>>>, port: u16| {
        if let Ok(mut guard) = child_slot.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
        let pk = format!("pkill -f scid={:08x} || true", params.scid);
        let _ = adb.run(&["-s", serial, "shell", pk.as_str()], 3000);
        if port != 0 {
            let fp = format!("tcp:{}", port);
            let _ = adb.run(&["-s", serial, "forward", "--remove", fp.as_str()], 3000);
        }
    };

    *shared.status.lock().unwrap() = Status::Connecting;
    let _ = ctx.request_repaint();
    shared.log("[conn] подключение…".into());

    // 1. push scrcpy-server.jar
    if let Err(e) = adb.push_server(serial, jar_path) {
        cleanup(child_slot, my_port);
        return Err(e);
    }
    shared.log("[conn] server.jar отправлен на устройство".into());
    if stop.load(Ordering::SeqCst) || superseded() {
        cleanup(child_slot, my_port);
        return Err(anyhow::anyhow!(ABORTED_MSG));
    }

    // 1.2 подчистить застрявшие forward прошлых попыток (они копятся после
    // аварийных завершений и мешают; свои мы потом снимаем по порт-у)
    let _ = adb.run(&["-s", serial, "forward", "--remove-all"], 4000);

    // 1.5 прибить зомби-серверы прошлых сессий (держат MediaCodec и не дают
    // новому серверу открыть сокет)
    let _ = adb.run(&["-s", serial, "shell", "pkill -f com.genymobile.scrcpy || true"], 4000);
    if stop.load(Ordering::SeqCst) || superseded() {
        cleanup(child_slot, my_port);
        return Err(anyhow::anyhow!(ABORTED_MSG));
    }

    // 2. adb forward: локальный порт -> localabstract:scrcpy_<scid>
    let socket_name = format!("{}{:08x}", SOCKET_PREFIX, params.scid);
    let port = match adb.forward(serial, &socket_name) {
        Ok(p) => p,
        Err(e) => {
            cleanup(child_slot, my_port);
            return Err(e);
        }
    };
    my_port = port;
    shared.log(format!("[conn] forward tcp:{} -> {}", port, socket_name));

    // 3. запустить сервер на устройстве
    let mut child = match adb.spawn_server(serial, params) {
        Ok(c) => c,
        Err(e) => {
            cleanup(child_slot, my_port);
            return Err(e);
        }
    };
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take();
    *child_slot.lock().unwrap() = Some(child);
    let shared2 = shared.clone();
    std::thread::spawn(move || drain_server_output(shared2, stdout, stderr));

    // 4. подключиться к видео-сокету (сервер шлёт dummy-байт на первый сокет).
    // Бюджет по ВРЕМЕНИ (не по итерациям): adb может держать соединение,
    // а сервер TECNO стартует до ~25 с.
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + Duration::from_secs(35);
    let mut video: Option<TcpStream> = None;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) || superseded() {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!(ABORTED_MSG));
        }
        if let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
            let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
            let mut dummy = [0u8; 1];
            match s.read(&mut dummy) {
                Ok(1) => {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = s.set_nodelay(true);
                    video = Some(s);
                    break;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    let mut video = match video {
        Some(v) => v,
        None => {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!(
                "сервер не открыл видео-сокет за 35 с (зомби-процессы убиты, попробуй ещё раз)"
            ));
        }
    };
    shared.log("[conn] видео-сокет подключён".into());

    // 5a. audio-сокет (второй accept; сервер ждёт его до control)
    let audio_stream = match TcpStream::connect_timeout(&addr, Duration::from_millis(2000)) {
        Ok(a) => {
            let _ = a.set_nodelay(true);
            let _ = a.set_read_timeout(Some(Duration::from_secs(10)));
            Some(a)
        }
        Err(e) => {
            shared.log(format!("[conn] аудио-сокет недоступен: {e}"));
            None
        }
    };

    // 5b. control-сокет (третий accept; dummy-байта на нём нет)
    let mut control: Option<TcpStream> = None;
    for _ in 0..100 {
        if stop.load(Ordering::SeqCst) || superseded() {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!(ABORTED_MSG));
        }
        if let Ok(s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
            let _ = s.set_nodelay(true);
            control = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let control = match control {
        Some(c) => c,
        None => {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!("control-сокет не подключился"));
        }
    };
    // v3 сервер декодирует скролл с множителем ×16 — компенсируем делителем
    let scroll_div = if params.version.starts_with('3') { 16.0 } else { 1.0 };
    std::thread::spawn(move || control_writer_stream(control, rx, scroll_div));
    shared.log("[conn] control-сокет подключён".into());

    // поток звука: opus -> rodio
    if let Some(audio) = audio_stream {
        let shared_audio = shared.clone();
        std::thread::spawn(move || audio_loop(audio, shared_audio));
        shared.log("[conn] аудио-поток запущен".into());
    }

    // 6. метаданные: имя устройства (64 байта) + размер видео (w:u16, h:u16
    //    — при send_frame_meta=false сервер шлёт компактный заголовок,
    //    как в py-scrcpy-client)
    let mut name_buf = [0u8; DEVICE_NAME_LEN];
    match read_full(&mut video, &mut name_buf) {
        Ok(()) => {
            let name = String::from_utf8_lossy(&name_buf)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            *shared.device_name.lock().unwrap() = name.clone();
            shared.log(format!("[conn] устройство: {}", name));
        }
        Err(e) => {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!("не удалось прочитать имя устройства: {:?}", e));
        }
    }

    // заголовок потока: codec_id(4) + width(4) + height(4) — шлётся
    // независимо от send_frame_meta (она убирает только per-packet PTS)
    let mut hdr12 = [0u8; 12];
    if let Err(e) = read_full(&mut video, &mut hdr12) {
        cleanup(child_slot, my_port);
        return Err(anyhow::anyhow!("не удалось прочитать заголовок видео: {:?}", e));
    }
    let codec_id = u32::from_be_bytes([hdr12[0], hdr12[1], hdr12[2], hdr12[3]]);
    if codec_id != 0x68323634 {
        cleanup(child_slot, my_port);
        return Err(anyhow::anyhow!(
            "ожидается h264, получен кодек {:08x}",
            codec_id
        ));
    }
    let vw = u32::from_be_bytes([hdr12[4], hdr12[5], hdr12[6], hdr12[7]]);
    let vh = u32::from_be_bytes([hdr12[8], hdr12[9], hdr12[10], hdr12[11]]);
    if !(64..=8192).contains(&vw) || !(64..=8192).contains(&vh) {
        cleanup(child_slot, my_port);
        return Err(anyhow::anyhow!("странный размер видео: {}x{}", vw, vh));
    }
    *shared.video_size.lock().unwrap() = (vw, vh);
    shared.log(format!(
        "[conn] размер видео: {}x{} (сервер v{})",
        vw, vh, params.version
    ));

    // 7. цикл декодирования — ПАКЕТНЫЙ РЕЖИМ (send_frame_meta=true,
    // дефолт сервера). Проверен часами работы на 61 fps. Поворот контента
    // не роняет энкодер благодаря lock_video_orientation (фикс TECNO).
    *shared.status.lock().unwrap() = Status::Streaming;
    let _ = ctx.request_repaint();

    // NoFlush: авто-флаш после каждого декода сбрасывает состояние декодера
    // (доки openh264-rs) — проверено часами в этом режиме.
    let mut decoder = match Decoder::with_api_config(
        OpenH264API::from_source(),
        DecoderConfig::new().flush_after_decode(Flush::NoFlush),
    ) {
        Ok(d) => d,
        Err(e) => {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!("не удалось создать декодер: {}", e));
        }
    };

    let mut config: Vec<u8> = Vec::new();
    let mut frame_times: VecDeque<Instant> = VecDeque::new();
    let mut version: u64 = 0;
    let mut bytes_window: u64 = 0;
    let mut bytes_at = Instant::now();
    let mut bitrate_kbps: u64 = 0;
    let mut consecutive_errs: u32 = 0;

    loop {
        if stop.load(Ordering::SeqCst) {
            cleanup(child_slot, my_port);
            return Ok(());
        }
        if superseded() {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!(ABORTED_MSG));
        }

        // заголовок пакета: 8 байт PTS+флаги, 4 байта длина
        let mut header = [0u8; PACKET_HEADER];
        match read_full(&mut video, &mut header) {
            Ok(()) => {}
            Err(ReadErr::TimedOut) => continue, // экран статичен — кадров нет
            Err(e) => {
                cleanup(child_slot, my_port);
                return Err(anyhow::anyhow!("видеопоток прерван: {:?}", e));
            }
        }
        let pts_flags = u64::from_be_bytes([
            header[0], header[1], header[2], header[3],
            header[4], header[5], header[6], header[7],
        ]);
        let len = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
        if len == 0 || len > (16 << 20) {
            cleanup(child_slot, my_port);
            return Err(anyhow::anyhow!("странный размер пакета: {}", len));
        }

        let mut payload = vec![0u8; len];
        match read_full(&mut video, &mut payload) {
            Ok(()) => {}
            Err(ReadErr::TimedOut) => continue,
            Err(e) => {
                cleanup(child_slot, my_port);
                return Err(anyhow::anyhow!("видеопоток прерван: {:?}", e));
            }
        }
        bytes_window += (PACKET_HEADER + len) as u64;

        // config-пакет (SPS/PPS) приклеиваем к следующему фрейму
        if pts_flags & PACKET_FLAG_CONFIG != 0 {
            config = payload;
            continue;
        }

        let mut packet = std::mem::take(&mut config);
        packet.extend_from_slice(&payload);

        let t0 = Instant::now();
        let decoded = match decoder.decode(&packet) {
            Ok(d) => {
                consecutive_errs = 0;
                d
            }
            Err(e) => {
                consecutive_errs += 1;
                if consecutive_errs <= 3 || consecutive_errs % 100 == 0 {
                    shared.log(format!(
                        "[decode] ошибка ({} подряд): {} — ждём keyframe",
                        consecutive_errs, e
                    ));
                }
                if consecutive_errs > 200 {
                    cleanup(child_slot, my_port);
                    return Err(anyhow::anyhow!(
                        "декодер не смог восстановиться после {} ошибок: {}",
                        consecutive_errs, e
                    ));
                }
                continue;
            }
        };
        let decode_ms = t0.elapsed().as_secs_f32() * 1000.0;

        if let Some(yuv) = decoded {
            let (w, h) = yuv.dimensions();
            if w > 0 && h > 0 {
                let mut rgba = vec![0u8; w * h * 4];
                yuv.write_rgba8(&mut rgba);
                version += 1;
                {
                    let mut vs = shared.video_size.lock().unwrap();
                    if vs.0 != w as u32 || vs.1 != h as u32 {
                        *vs = (w as u32, h as u32);
                        shared.log(format!("[conn] новый размер кадра: {}x{}", w, h));
                    }
                }
                *shared.frame.lock().unwrap() = Some(Frame {
                    rgba: Arc::new(rgba),
                    w,
                    h,
                    version,
                });

                let now = Instant::now();
                frame_times.push_back(now);
                while let Some(front) = frame_times.front() {
                    if now.duration_since(*front) > Duration::from_secs(1) {
                        frame_times.pop_front();
                    } else {
                        break;
                    }
                }
                let fps = frame_times.len() as u32;
                {
                    let mut st = shared.stats.lock().unwrap();
                    st.fps = fps;
                    st.decode_ms = decode_ms;
                    st.count += 1;
                }
                if bytes_at.elapsed() > Duration::from_secs(1) {
                    let secs = bytes_at.elapsed().as_secs().max(1) as u64;
                    bitrate_kbps = bytes_window * 8 / 1000 / secs;
                    bytes_window = 0;
                    bytes_at = now;
                    shared.stats.lock().unwrap().bitrate_kbps = bitrate_kbps;
                }
                let _ = ctx.request_repaint();
            }
        }
    }
}

/// Приём и воспроизведение звука: raw PCM 48кГц stereo -> rodio.
fn audio_loop(mut stream: TcpStream, shared: Arc<Shared>) {
    let (_out_stream, handle) = match rodio::OutputStream::try_default() {
        Ok(x) => x,
        Err(e) => {
            shared.log(format!("[audio] не удалось открыть аудиовыход: {e}"));
            return;
        }
    };
    let sink = match rodio::Sink::try_new(&handle) {
        Ok(s) => s,
        Err(e) => {
            shared.log(format!("[audio] sink: {e}"));
            return;
        }
    };
    sink.set_volume(1.0);

    // Аудио-поток начинается с 4-байтового codec id ("raw\0" = 0x00726177
    // для PCM — Streamer.writeAudioHeader), и только потом идут пакеты
    // [pts+flags u64, len u32]. Без этого чтения вся разметка съезжает.
    let mut codec_id = [0u8; 4];
    if read_full(&mut stream, &mut codec_id).is_err() {
        shared.log("[audio] поток закрыт (codec id)".into());
        return;
    }
    let codec = u32::from_be_bytes(codec_id);
    shared.log(format!(
        "[audio] codec id: {codec:#010x} (raw = 0x00726177)"
    ));

    let mut meta = [0u8; 12];
    let mut first_packet = true;
    loop {
        if read_full(&mut stream, &mut meta).is_err() {
            shared.log("[audio] поток закрыт (заголовок)".into());
            return; // аудио закрылось — видео продолжается
        }
        let pts_flags = u64::from_be_bytes([meta[0], meta[1], meta[2], meta[3], meta[4], meta[5], meta[6], meta[7]]);
        let len = u32::from_be_bytes([meta[8], meta[9], meta[10], meta[11]]) as usize;
        if len == 0 || len > (1 << 20) {
            shared.log(format!(
                "[audio] странный размер пакета: {} (pts_flags={pts_flags:#x})",
                len
            ));
            return;
        }
        let mut payload = vec![0u8; len];
        if read_full(&mut stream, &mut payload).is_err() {
            shared.log("[audio] поток закрыт (данные)".into());
            return;
        }
        // codec-config пакет пропускаем (для raw PCM не нужен)
        if pts_flags & PACKET_FLAG_CONFIG != 0 {
            continue;
        }
        if first_packet {
            first_packet = false;
            shared.log(format!("[audio] первый звуковой пакет: {} байт", len));
        }
        // RAW PCM 16-bit LE stereo 48кГц — чередование L,R; rodio ждёт
        // именно чередующиеся сэмплы по каналам, без даунмикса.
        let mut samples = vec![0i16; payload.len() / 2];
        for (i, b) in payload.chunks_exact(2).enumerate() {
            samples[i] = i16::from_le_bytes([b[0], b[1]]);
        }
        sink.append(rodio::buffer::SamplesBuffer::new(2, 48000, samples));
    }
}

fn control_writer_stream(mut stream: TcpStream, rx: mpsc::Receiver<ControlMsg>, scroll_div: f32) {
    use std::io::Write;
    for msg in rx {
        let mut buf = Vec::with_capacity(64);
        match msg {
            ControlMsg::Scroll { x, y, sw, sh, vscroll } => {
                ControlMsg::Scroll { x, y, sw, sh, vscroll: vscroll / scroll_div }
                    .encode_into(&mut buf);
            }
            other => other.encode_into(&mut buf),
        }
        if stream.write_all(&buf).and_then(|_| stream.flush()).is_err() {
            break;
        }
    }
}

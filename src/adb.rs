// Обёртка над adb.exe: устройства, push сервера, forward, запуск app_process.
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Adb {
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub model: String,
}

impl Adb {
    /// Ищем adb по типовым местам.
    pub fn find() -> Result<Adb> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(d) = &exe_dir {
            candidates.push(d.join("adb.exe"));
            candidates.push(d.join("tools").join("platform-tools").join("adb.exe"));
            candidates.push(d.join("../tools/platform-tools/adb.exe"));
            candidates.push(d.join("../../tools/platform-tools/adb.exe"));
            candidates.push(d.join("../assets/adb/adb.exe"));
        }
        candidates.push(PathBuf::from("adb.exe")); // из PATH

        for c in &candidates {
            if c.is_file() || c == &PathBuf::from("adb.exe") {
                let adb = Adb { path: c.clone() };
                if adb.run(&["version"], 5000).is_ok() {
                    return Ok(adb);
                }
            }
        }
        bail!("adb.exe не найден. Укажи путь в Настройках.")
    }

    /// Запустить adb и получить stdout с ограничением по времени.
    pub fn run(&self, args: &[&str], timeout_ms: u64) -> Result<String> {
        let mut child = Command::new(&self.path)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("не удалось запустить {:?}", self.path))?;

        let start = Instant::now();
        loop {
            match child.try_wait()? {
                Some(_) => break,
                None => {
                    if start.elapsed() > Duration::from_millis(timeout_ms) {
                        let _ = child.kill();
                        bail!("adb {} не ответил за {} мс", args.join(" "), timeout_ms);
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }

        let mut out = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut out);
        }
        Ok(out)
    }

    /// Список подключённых устройств.
    pub fn devices(&self) -> Result<Vec<Device>> {
        let out = self.run(&["devices", "-l"], 8000)?;
        let mut devices = Vec::new();
        for line in out.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let serial = parts.next().unwrap_or("").to_string();
            let state = parts.next().unwrap_or("").to_string();
            let model = parts
                .find(|p| p.starts_with("model:"))
                .map(|p| p.trim_start_matches("model:").to_string())
                .unwrap_or_else(|| serial.clone());
            if !serial.is_empty() {
                devices.push(Device { serial, state, model });
            }
        }
        Ok(devices)
    }

    pub fn push_server(&self, serial: &str, jar: &Path) -> Result<()> {
        let jar_str = jar
            .to_str()
            .ok_or_else(|| anyhow!("путь к scrcpy-server.jar нечитаем"))?;
        self.run(&["-s", serial, "push", jar_str, "/data/local/tmp/scrcpy-server.jar"], 30000)?;
        Ok(())
    }

    /// Пробросить локальный порт на абстрактный сокет; вернёт выбранный порт.
    pub fn forward(&self, serial: &str, socket_name: &str) -> Result<u16> {
        let out = self.run(
            &["-s", serial, "forward", "tcp:0", &format!("localabstract:{}", socket_name)],
            8000,
        )?;
        let port: u16 = out
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| anyhow!("adb не вернул порт forward: {:?}", out))?;
        Ok(port)
    }

    pub fn remove_forward(&self, serial: &str, port: u16) {
        let _ = self.run(&["-s", serial, "forward", "--remove", &format!("tcp:{}", port)], 5000);
    }

    /// Запустить сервер scrcpy на устройстве (stdout/stderr будут собраны потоками).
    pub fn spawn_server(
        &self,
        serial: &str,
        params: &ServerParams,
    ) -> Result<Child> {
        let mut cmd = Command::new(&self.path);
        cmd.arg("-s")
            .arg(serial)
            .arg("shell")
            .arg(format!("CLASSPATH={}", crate::connection::DEVICE_SERVER_PATH))
            .arg("app_process")
            .arg("/")
            .arg("com.genymobile.scrcpy.Server")
            .arg(params.version)
            .arg(format!("scid={:08x}", params.scid))
            .arg("log_level=info")
            .arg("audio=true")
            .arg("audio_codec=raw") // PCM 48кГц stereo — без декодера
            .arg("tunnel_forward=true")
            .arg(format!("video_bit_rate={}", params.bitrate_bps));

        if let Some((w, h)) = params.new_display {
            cmd.arg(format!("new_display={}x{}", w, h));
        }
        if let Some(fps) = params.max_fps {
            cmd.arg(format!("max_fps={}", fps));
        }
        if let Some(max_size) = params.max_size {
            cmd.arg(format!("max_size={}", max_size));
        }
        if params.show_touches {
            cmd.arg("show_touches=true");
        }
        if params.stay_awake {
            cmd.arg("stay_awake=true");
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        Ok(cmd.spawn()?)
    }

    /// Скриншот экрана телефона -> PNG-файл на ПК.
    pub fn screencap(&self, serial: &str, out: &Path) -> Result<()> {
        let file = std::fs::File::create(out)?;
        let mut child = Command::new(&self.path)
            .args(["-s", serial, "exec-out", "screencap", "-p"])
            .stdout(Stdio::from(file))
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            match child.try_wait()? {
                Some(_) => break,
                None => {
                    if start.elapsed() > Duration::from_secs(20) {
                        let _ = child.kill();
                        bail!("screencap завис");
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
        Ok(())
    }
}

/// Параметры запуска scrcpy-сервера.
pub struct ServerParams {
    pub scid: u32,
    /// версия сервера — первый аргумент, строго сверяется на устройстве
    pub version: &'static str,
    /// виртуальный дисплей WxH (scrcpy 3.x new_display) — None = физический экран
    pub new_display: Option<(u32, u32)>,
    pub bitrate_bps: u32,
    pub max_fps: Option<u32>,
    pub max_size: Option<u32>,
    pub show_touches: bool,
    pub stay_awake: bool,
}

impl ServerParams {
    pub fn new(
        version: &'static str,
        new_display: Option<(u32, u32)>,
        bitrate_bps: u32,
        max_fps: Option<u32>,
        max_size: Option<u32>,
        show_touches: bool,
        stay_awake: bool,
    ) -> Self {
        // scid — случайный идентификатор сессии. ВАЖНО: сервер парсит его как
        // Integer.parseInt(hex, 16) — ЗНАКОВЫЙ int, поэтому старший бит
        // обязан быть нулём (иначе NumberFormatException и смерть сервера).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
            .unwrap_or(0x1234_5678);
        let scid = ((nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as u32) & 0x7FFF_FFFF;
        Self { scid, version, new_display, bitrate_bps, max_fps, max_size, show_touches, stay_awake }
    }
}

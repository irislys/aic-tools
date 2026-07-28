pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(&self) -> &'static str {
        match self {
            Level::Info => "I",
            Level::Warn => "W",
            Level::Error => "E",
        }
    }
}

pub fn init() {}

pub fn log(level: Level, msg: impl AsRef<str>) {
    let ts = timestamp();
    let line = format!("{ts} [{tag}] {msg}", tag = level.tag(), msg = msg.as_ref());
    eprintln!("{line}");
}

pub fn info(msg: impl AsRef<str>) {
    log(Level::Info, msg);
}

pub fn warn(msg: impl AsRef<str>) {
    log(Level::Warn, msg);
}

pub fn error(msg: impl AsRef<str>) {
    log(Level::Error, msg);
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = now.as_secs() as i64;
    let millis = now.subsec_millis();

    let days = total_secs / 86_400;
    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let h = ((total_secs % 86_400) / 3600) as u32;
    let mi = ((total_secs % 3600) / 60) as u32;
    let s = (total_secs % 60) as u32;

    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}.{millis:03}",)
}

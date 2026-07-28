use std::path::Path;

use crate::mem::Process;

pub const SUPPORTED_CONTENT_VERSIONS: &[&str] = &["0.29j"];
const MAX_GLOBAL_GAME_MANAGERS_BYTES: u64 = 64 * 1024 * 1024;

pub fn is_supported(version: &str) -> bool {
    let v = version.trim();
    SUPPORTED_CONTENT_VERSIONS.contains(&v)
}

pub fn detect_content_version(proc: &Process) -> Result<String, String> {
    let exe = proc
        .exe_path()
        .ok_or_else(|| "无法取得进程路径".to_string())?;
    let exe = std::fs::canonicalize(exe).map_err(|e| format!("无法规范化进程路径: {e}"))?;
    if !exe
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("AliceInCradle.exe"))
    {
        return Err(format!("目标可执行文件名异常: {}", exe.display()));
    }
    let data_dir = exe
        .parent()
        .map(|p| p.join("AliceInCradle_Data"))
        .ok_or_else(|| "进程路径无效".to_string())?;
    let data_dir = std::fs::canonicalize(data_dir).map_err(|e| format!("游戏数据目录无效: {e}"))?;
    let ggm = data_dir.join("globalgamemanagers");
    if !ggm.is_file() {
        return Err(format!("未找到 {}", ggm.display()));
    }
    parse_version_from_globalgamemanagers(&ggm)
}

fn parse_version_from_globalgamemanagers(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("读取 globalgamemanagers 元数据失败: {e}"))?
        .len();
    if size > MAX_GLOBAL_GAME_MANAGERS_BYTES {
        return Err(format!(
            "globalgamemanagers 过大 ({size} bytes, 上限 {MAX_GLOBAL_GAME_MANAGERS_BYTES})"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取 globalgamemanagers 失败: {e}"))?;
    let mut cands = extract_unity_len_strings(&bytes);
    cands.retain(|(_, v)| is_plausible_content_version(v));
    if let Some(v) = pick_best_near_marker(&bytes, &cands, b"AliceInCradle") {
        return Ok(v);
    }
    if let Some(v) = pick_best_near_marker(&bytes, &cands, b"NanameHacha") {
        return Ok(v);
    }
    pick_best_version(cands).ok_or_else(|| "globalgamemanagers 中未解析到游戏版本".into())
}

fn extract_unity_len_strings(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        let ln = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        if (1..=24).contains(&ln) && i + 4 + ln <= bytes.len() {
            let s = &bytes[i + 4..i + 4 + ln];
            if s.iter().all(|c| (0x20..0x7f).contains(c)) {
                if let Ok(t) = std::str::from_utf8(s) {
                    out.push((i + 4, t.to_string()));
                }
                i += 4 + ln;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn pick_best_near_marker(bytes: &[u8], cands: &[(usize, String)], marker: &[u8]) -> Option<String> {
    let at = find_slice(bytes, marker)?;
    let mut near: Vec<&(usize, String)> = cands
        .iter()
        .filter(|(off, _)| {
            let d = (*off as isize - at as isize).unsigned_abs();
            d < 2048
        })
        .collect();
    near.sort_by_key(|(off, v)| {
        let dist = (*off as isize - at as isize).unsigned_abs() as i32;
        version_score(v) * -1000 + dist
    });
    near.first().map(|(_, v)| v.clone())
}

fn pick_best_version(cands: Vec<(usize, String)>) -> Option<String> {
    let mut v: Vec<_> = cands.into_iter().map(|(_, s)| s).collect();
    v.sort_by_key(|s| -version_score(s));
    v.into_iter().next()
}

fn version_score(v: &str) -> i32 {
    let mut score = 0i32;
    if v.contains('.') {
        score += 10;
    }
    if v.chars().any(|c| c.is_ascii_alphabetic()) {
        score += 5;
    }
    if v.starts_with('0') {
        score += 4;
    }
    if v == "1.0" || v == "1.0.0" {
        score -= 20;
    }
    if v.len() <= 8 {
        score += 3;
    }
    score
}

fn find_slice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn looks_like_version_token(s: &str) -> bool {
    if s.chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c.is_ascii_alphabetic())
        && s.contains('.')
        && s.chars().any(|c| c.is_ascii_digit())
    {
        if s.starts_with("20") && s.len() >= 6 {
            return false;
        }
        if s.contains('f')
            && s.chars().filter(|c| c.is_ascii_digit()).count() >= 4
            && s.chars().filter(|c| *c == '.').count() >= 2
        {
            return false;
        }
        return true;
    }
    false
}

fn is_plausible_content_version(s: &str) -> bool {
    looks_like_version_token(s) && s.len() <= 12
}

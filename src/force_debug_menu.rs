use std::sync::atomic::{AtomicBool, Ordering};

use crate::log;
use crate::mem::Process;

pub const IL_ORIG: &[u8] = &[
    0x7E, 0x77, 0x1D, 0x00, 0x04, 0x2C, 0x07, 0x7E, 0x7E, 0x1D, 0x00, 0x04, 0x2D, 0x03, 0x16, 0x2B,
    0x01, 0x17, 0x80, 0x71, 0x00, 0x00, 0x04, 0x7E,
];

pub const IL_PATCH: &[u8] = &[
    0x17, 0x80, 0x71, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const SCAN_CHUNK: usize = 0x1_0000;

#[derive(Clone, Debug)]
pub struct F7Patch {
    pub addr: u64,
    pub applied: bool,
}

impl F7Patch {
    pub fn empty() -> Self {
        Self {
            addr: 0,
            applied: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PatchOutcome {
    Applied { addr: u64 },
    AlreadyPatched { addr: u64 },
}

#[derive(Clone, Debug)]
pub enum PatchError {
    Cancelled,
    NotFound,
    Multiple { count: usize, sample: Vec<u64> },
    Unexpected { addr: u64, bytes: Vec<u8> },
    WriteFailed { addr: u64 },
    VerifyFailed { addr: u64 },
}

impl PatchError {
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "扫描已取消".into(),
            Self::NotFound => "未找到 initDebugger IL 特征（游戏未就绪或版本不匹配）".into(),
            Self::Multiple { count, sample } => {
                let addrs: Vec<String> = sample.iter().map(|a| format!("{a:#x}")).collect();
                format!(
                    "IL 特征命中 {count} 处，拒绝写入（样本: {}）",
                    addrs.join(", ")
                )
            }
            Self::Unexpected { addr, bytes } => {
                format!("地址 {addr:#x} 字节非预期: {}", hex_bytes(bytes))
            }
            Self::WriteFailed { addr } => format!("写入失败 addr={addr:#x}"),
            Self::VerifyFailed { addr } => format!("写入后校验失败 addr={addr:#x}"),
        }
    }
}

pub fn hex_bytes(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn region_priority(base: u64, size: u64) -> i32 {
    let mut p = 0;
    if (0x10_0000..0x200_0000).contains(&size) {
        p += 3;
    }
    if (0x1_0000_0000..0x8000_0000_0000).contains(&base) {
        p += 2;
    }
    if size < 0x1000 {
        p -= 5;
    }
    p
}

pub fn find_il(proc: &Process, stop: &AtomicBool) -> Result<(u64, bool), PatchError> {
    let mut regions = proc.readable_regions();
    regions.sort_by_key(|&(b, s)| std::cmp::Reverse(region_priority(b, s)));

    let mut orig_hits: Vec<u64> = Vec::new();
    let mut patch_hits: Vec<u64> = Vec::new();
    let mut buf = vec![0u8; SCAN_CHUNK + IL_ORIG.len()];

    for (base, size) in regions {
        if stop.load(Ordering::Relaxed) {
            return Err(PatchError::Cancelled);
        }
        if size < IL_ORIG.len() as u64 {
            continue;
        }
        let scan_size = size.min(0x30_0000);
        let mut off = 0u64;
        while off < scan_size {
            if stop.load(Ordering::Relaxed) {
                return Err(PatchError::Cancelled);
            }
            let want = ((scan_size - off) as usize).min(buf.len());
            let addr = base + off;
            if !proc.read_bytes(addr, &mut buf[..want]) {
                off = off.saturating_add(SCAN_CHUNK as u64);
                continue;
            }
            let limit = want.saturating_sub(IL_ORIG.len());
            let mut i = 0usize;
            while i <= limit {
                if (i & 0xFFF) == 0 && stop.load(Ordering::Relaxed) {
                    return Err(PatchError::Cancelled);
                }
                if buf[i..i + IL_ORIG.len()] == *IL_ORIG {
                    orig_hits.push(addr + i as u64);
                    i += IL_ORIG.len();
                    continue;
                }
                if buf[i..i + IL_PATCH.len()] == *IL_PATCH {
                    let ok_tail = if i + IL_PATCH.len() < want {
                        buf[i + IL_PATCH.len()] == 0x7E
                    } else {
                        true
                    };
                    if ok_tail {
                        patch_hits.push(addr + i as u64);
                    }
                    i += IL_PATCH.len();
                    continue;
                }
                i += 1;
            }
            off = off.saturating_add(SCAN_CHUNK as u64);
        }
        if orig_hits.len() + patch_hits.len() > 8 {
            break;
        }
    }

    if orig_hits.len() == 1 {
        return Ok((orig_hits[0], false));
    }
    if orig_hits.is_empty() && patch_hits.len() == 1 {
        return Ok((patch_hits[0], true));
    }
    if orig_hits.is_empty() && patch_hits.is_empty() {
        return Err(PatchError::NotFound);
    }
    let mut sample = orig_hits.clone();
    sample.extend(patch_hits.iter().copied());
    sample.truncate(4);
    Err(PatchError::Multiple {
        count: orig_hits.len() + patch_hits.len(),
        sample,
    })
}

pub fn apply_at(proc: &Process, addr: u64) -> Result<PatchOutcome, PatchError> {
    let mut cur = vec![0u8; IL_PATCH.len()];
    if !proc.read_bytes(addr, &mut cur) {
        return Err(PatchError::WriteFailed { addr });
    }
    if cur == IL_PATCH {
        return Ok(PatchOutcome::AlreadyPatched { addr });
    }

    let mut full = vec![0u8; IL_ORIG.len()];
    if !(proc.read_bytes(addr, &mut full) && full == IL_ORIG) {
        return Err(PatchError::Unexpected { addr, bytes: cur });
    }

    if !proc.write_bytes(addr, IL_PATCH) && !proc.write_code_bytes(addr, IL_PATCH) {
        return Err(PatchError::WriteFailed { addr });
    }

    let mut verify = vec![0u8; IL_PATCH.len()];
    if !proc.read_bytes(addr, &mut verify) || verify != IL_PATCH {
        return Err(PatchError::VerifyFailed { addr });
    }
    Ok(PatchOutcome::Applied { addr })
}

pub fn ensure_patched(
    proc: &Process,
    stop: &AtomicBool,
    cached_addr: Option<u64>,
) -> Result<PatchOutcome, PatchError> {
    if let Some(addr) = cached_addr
        && !stop.load(Ordering::Relaxed)
    {
        match apply_at(proc, addr) {
            Ok(outcome) => return Ok(outcome),
            Err(PatchError::Unexpected { .. }) | Err(PatchError::WriteFailed { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    let (addr, _) = find_il(proc, stop)?;
    apply_at(proc, addr)
}

pub fn apply(proc: &Process, prev: &F7Patch) -> Result<F7Patch, String> {
    let stop = AtomicBool::new(false);
    let cached = (prev.addr != 0).then_some(prev.addr);
    let mut last_err = String::new();

    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        match ensure_patched(proc, &stop, cached) {
            Ok(PatchOutcome::Applied { addr }) => {
                log::info(format!(
                    "F7 调试菜单补丁已写入 addr={addr:#x} patch={}",
                    hex_bytes(IL_PATCH)
                ));
                return Ok(F7Patch {
                    addr,
                    applied: true,
                });
            }
            Ok(PatchOutcome::AlreadyPatched { addr }) => {
                log::info(format!("F7 调试菜单补丁已存在 addr={addr:#x}"));
                return Ok(F7Patch {
                    addr,
                    applied: true,
                });
            }
            Err(e) => {
                last_err = e.message();
                log::warn(format!("F7 补丁尝试 {}/5: {last_err}", attempt + 1));
                if matches!(e, PatchError::Multiple { .. } | PatchError::Cancelled) {
                    break;
                }
            }
        }
    }
    Err(last_err)
}

pub fn status(proc: &Process, bak: &F7Patch) -> String {
    if !bak.applied || bak.addr == 0 {
        return "关".into();
    }
    let mut cur = vec![0u8; IL_PATCH.len()];
    if !proc.read_bytes(bak.addr, &mut cur) {
        return format!("地址不可读 (addr={:#x})", bak.addr);
    }
    if cur == IL_PATCH {
        format!("开 (addr={:#x})", bak.addr)
    } else {
        format!("状态未知 (addr={:#x})", bak.addr)
    }
}

use crate::mem::Process;

const CHUNK: usize = 0x10_0000;

pub fn utf16le_pattern(key: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(key.len() * 2 + 2);
    for b in key.bytes() {
        out.extend_from_slice(&[b, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

pub fn scan_pattern(proc: &Process, pat: &[u8], max_hits: usize) -> Vec<u64> {
    scan_with(proc, pat, max_hits, Process::writable_regions)
}

pub fn scan_pattern_readable(proc: &Process, pat: &[u8], max_hits: usize) -> Vec<u64> {
    scan_with(proc, pat, max_hits, Process::readable_regions)
}

fn scan_with(
    proc: &Process,
    pat: &[u8],
    max_hits: usize,
    regions: fn(&Process) -> Vec<(u64, u64)>,
) -> Vec<u64> {
    if pat.is_empty() || max_hits == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (base, size) in regions(proc) {
        if hits.len() >= max_hits {
            break;
        }
        scan_region(proc, base, size, pat, max_hits, &mut hits);
    }
    hits
}

fn scan_region(
    proc: &Process,
    base: u64,
    size: u64,
    pat: &[u8],
    max_hits: usize,
    hits: &mut Vec<u64>,
) {
    let mut off = 0u64;
    let mut prev_tail = Vec::new();
    while off < size && hits.len() < max_hits {
        let take = ((size - off) as usize).min(CHUNK);
        let mut buf = vec![0u8; take];
        if !proc.read_bytes(base + off, &mut buf) {
            off += take as u64;
            prev_tail.clear();
            continue;
        }
        let prev_len = prev_tail.len();
        let mut hay = std::mem::take(&mut prev_tail);
        hay.extend_from_slice(&buf);
        let start_addr = (base + off).saturating_sub(prev_len as u64);
        find_in_buffer(&hay, pat, start_addr, max_hits, hits);
        let keep = pat.len().saturating_sub(1);
        prev_tail = if keep > 0 && hay.len() >= keep {
            hay[hay.len() - keep..].to_vec()
        } else {
            hay
        };
        off += take as u64;
    }
}

fn find_in_buffer(hay: &[u8], pat: &[u8], base_addr: u64, max_hits: usize, hits: &mut Vec<u64>) {
    if pat.len() > hay.len() {
        return;
    }
    let last = hay.len() - pat.len();
    let mut i = 0usize;
    while i <= last && hits.len() < max_hits {
        if hay[i..i + pat.len()] == *pat {
            hits.push(base_addr + i as u64);
            i += pat.len();
        } else {
            i += 1;
        }
    }
}

pub fn scan_ptr_refs(proc: &Process, target: u64, max_hits: usize) -> Vec<u64> {
    if target == 0 || max_hits == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (base, size) in proc.writable_regions() {
        if hits.len() >= max_hits {
            break;
        }
        scan_region_ptr(proc, base, size, &[target], max_hits, &mut |addr, _| {
            hits.push(addr);
        });
    }
    hits
}

pub fn scan_multi_ptr_refs(proc: &Process, targets: &[u64], max_hits: usize) -> Vec<(u64, usize)> {
    if targets.is_empty() || max_hits == 0 {
        return Vec::new();
    }
    let clean: Vec<u64> = targets.iter().copied().filter(|&t| t != 0).collect();
    if clean.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (base, size) in proc.writable_regions() {
        if hits.len() >= max_hits {
            break;
        }
        scan_region_ptr(proc, base, size, &clean, max_hits, &mut |addr, ti| {
            hits.push((addr, ti));
        });
    }
    hits
}

fn scan_region_ptr(
    proc: &Process,
    base: u64,
    size: u64,
    targets: &[u64],
    max_hits: usize,
    on_hit: &mut dyn FnMut(u64, usize),
) {
    let mut hit_count = 0usize;
    let mut off = 0u64;
    while off < size {
        let take = (((size - off) as usize).min(CHUNK) / 8) * 8;
        if take == 0 {
            break;
        }
        let mut buf = vec![0u8; take];
        if !proc.read_bytes(base + off, &mut buf) {
            off += take as u64;
            continue;
        }
        for (i, chunk) in buf.chunks_exact(8).enumerate() {
            let val = u64::from_le_bytes(chunk.try_into().unwrap());
            if let Some(ti) = targets.iter().position(|&t| t == val) {
                on_hit(base + off + (i * 8) as u64, ti);
                hit_count += 1;
                if hit_count >= max_hits {
                    return;
                }
            }
        }
        off += take as u64;
    }
}

pub fn mono_string_from_chars(chars_addr: u64) -> u64 {
    chars_addr.saturating_sub(0x14)
}

pub fn read_mono_string(proc: &Process, obj: u64) -> Option<String> {
    if obj == 0 || !proc.looks_like_user_ptr(obj) {
        return None;
    }
    let len = proc.read_u32(obj + 0x10)? as usize;
    if len > 256 {
        return None;
    }
    let mut buf = vec![0u8; len * 2];
    if !proc.read_bytes(obj + 0x14, &mut buf) {
        return None;
    }
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

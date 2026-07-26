use crate::aob::{
    mono_string_from_chars, read_mono_string, scan_multi_ptr_refs, scan_pattern,
    scan_pattern_readable, scan_ptr_refs, utf16le_pattern,
};
use crate::log;
use crate::mem::Process;
use crate::offsets::*;

#[derive(Clone, Debug)]
pub struct CaneTargets {
    pub nel_item: u64,
    pub cane_item: u64,
    pub pr_equip: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct JuiceTarget {
    pub nel_item: u64,
}

fn find_mono_string_objects(proc: &Process, key: &str) -> Vec<u64> {
    let pat = utf16le_pattern(key);
    let mut char_hits = scan_pattern(proc, &pat, 64);
    if char_hits.is_empty() {
        log::info("字符串未命中可写区域，回退全可读扫描");
        char_hits = scan_pattern_readable(proc, &pat, 64);
    }
    let mut objs: Vec<u64> = char_hits
        .into_iter()
        .map(mono_string_from_chars)
        .filter(|&obj| read_mono_string(proc, obj).as_deref() == Some(key))
        .collect();
    objs.sort_unstable();
    objs.dedup();
    objs
}

fn nel_from_strings(
    proc: &Process,
    strs: &[u64],
    price: i32,
    id_filter: Option<u16>,
) -> Option<u64> {
    if strs.is_empty() {
        return None;
    }
    for (r, ti) in scan_multi_ptr_refs(proc, strs, 1024) {
        let nel = r.saturating_sub(NEL_KEY);
        if !nel.is_multiple_of(8) || !proc.looks_like_user_ptr(nel) {
            continue;
        }
        let str_obj = strs[ti];
        if proc.read_u64(nel + NEL_KEY) != Some(str_obj) {
            continue;
        }
        let Some(p) = proc.read_u32(nel + NEL_PRICE).map(|v| v as i32) else {
            continue;
        };
        if p != price {
            continue;
        }
        let Some(got) = proc.read_u16(nel + NEL_ID) else {
            continue;
        };
        let id_ok = match id_filter {
            Some(want) => got == want,
            None => got > 100,
        };
        if !id_ok {
            continue;
        }
        log::info(format!("nel_from_strings 命中 nel={nel:#x} price={p}"));
        return Some(nel);
    }
    None
}

fn pr_from_cane(proc: &Process, cane: u64) -> Option<u64> {
    let mut best: Option<(u64, i32)> = None;
    for r in scan_ptr_refs(proc, cane, 1024) {
        let pe = r.saturating_sub(PE_SRC_CANE);
        if !pe.is_multiple_of(8) || !proc.looks_like_user_ptr(pe) {
            continue;
        }
        if proc.read_u64(pe + PE_SRC_CANE) != Some(cane) {
            continue;
        }
        let (Some(np), Some(nr)) = (
            proc.read_f32(pe + PE_NEAR_POWER),
            proc.read_f32(pe + PE_NEAR_REACH),
        ) else {
            continue;
        };
        if !np.is_finite() || !nr.is_finite() {
            log::warn(format!(
                "PrCaneEquip 候选 {pe:#x} 读到非有限值 np={np} nr={nr}，跳过"
            ));
            continue;
        }
        if !(0.05..=30.0).contains(&np) || !(0.05..=30.0).contains(&nr) {
            continue;
        }
        let mut g = [0u8; 1];
        if !proc.read_bytes(pe + PE_GRADE, &mut g) {
            continue;
        }
        let score = 100 - (g[0] as i32).min(10);
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((pe, score));
        }
    }
    if let Some((pe, _)) = best {
        log::info(format!("pr_from_cane 命中 pe={pe:#x}"));
    }
    best.map(|(a, _)| a)
}

pub fn find_beginner_cane(proc: &Process) -> Result<CaneTargets, String> {
    log::info("开始定位初学者法杖");
    eprintln!("扫描 cane_default...");
    let strs = find_mono_string_objects(proc, CANE_ITEM_KEY);
    if strs.is_empty() {
        log::warn("未找到 cane_default 字符串");
        return Err("未找到 cane_default，请先进入存档".into());
    }
    log::info(format!("字符串候选 {} 个", strs.len()));

    let nel = nel_from_strings(proc, &strs, CANE_PRICE, Some(CANE_ID))
        .ok_or_else(|| "未定位到 NelItem".to_string())?;
    eprintln!("NelItem={nel:#x}");

    eprintln!("远程 Mono 调用定位 CaneManager.DefaultCane...");
    let cane = crate::mono::find_default_cane(proc).map_err(|e| format!("Mono: {e}"))?;
    eprintln!("CaneItem={cane:#x}");

    eprintln!("反查 PrCaneEquip …");
    let pe = pr_from_cane(proc, cane);
    match pe {
        Some(p) => eprintln!("PrCaneEquip={p:#x}"),
        None => eprintln!("未找到 PrCaneEquip（可能未装备该杖；仍可改表数据）"),
    }

    Ok(CaneTargets {
        nel_item: nel,
        cane_item: cane,
        pr_equip: pe,
    })
}

pub fn find_noel_juice(proc: &Process) -> Result<JuiceTarget, String> {
    log::info("开始定位诺艾尔汁");
    eprintln!("扫描 mtr_noel_juice0...");
    let strs = find_mono_string_objects(proc, JUICE_ITEM_KEY);
    if strs.is_empty() {
        log::warn("未找到 mtr_noel_juice0 字符串");
        return Err("未找到 mtr_noel_juice0".into());
    }
    log::info(format!("字符串候选 {} 个", strs.len()));
    let nel = nel_from_strings(proc, &strs, JUICE_PRICE, None).ok_or_else(|| {
        log::warn("未定位到诺艾尔汁 NelItem");
        "未定位到诺艾尔汁 NelItem".to_string()
    })?;
    log::info(format!("诺艾尔汁定位完成 nel={nel:#x}"));
    eprintln!("诺艾尔汁={nel:#x}");
    Ok(JuiceTarget { nel_item: nel })
}

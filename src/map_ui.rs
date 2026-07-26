use crate::aob::read_mono_string;
use crate::find::{CaneTargets, JuiceTarget};
use crate::log;
use crate::mem::Process;
use crate::offsets::*;

#[derive(Clone, Debug)]
pub struct MapBackup {
    pub juice: u64,
    pub def_cane: u64,
    pub cat: u32,
    pub fn_detail: u64,
    pub key_ptr: u64,
    pub itemdata: u64,
    pub applied: bool,
}

impl MapBackup {
    pub fn empty() -> Self {
        Self {
            juice: 0,
            def_cane: 0,
            cat: 0,
            fn_detail: 0,
            key_ptr: 0,
            itemdata: 0,
            applied: false,
        }
    }
}

fn read_u64_or(proc: &Process, addr: u64, name: &str) -> Result<u64, String> {
    proc.read_u64(addr)
        .ok_or_else(|| format!("读取 {name} 失败 addr={addr:#x}"))
}

fn read_u32_or(proc: &Process, addr: u64, name: &str) -> Result<u32, String> {
    proc.read_u32(addr)
        .ok_or_else(|| format!("读取 {name} 失败 addr={addr:#x}"))
}

fn write_or(ok: bool, addr: u64, name: &str) -> Result<(), String> {
    ok.then_some(())
        .ok_or_else(|| format!("写入 {name} 失败 addr={addr:#x}"))
}

fn write_u32_or(proc: &Process, addr: u64, v: u32, name: &str) -> Result<(), String> {
    write_or(proc.write_u32(addr, v), addr, name)
}

fn write_u64_or(proc: &Process, addr: u64, v: u64, name: &str) -> Result<(), String> {
    write_or(proc.write_u64(addr, v), addr, name)
}

pub fn apply_juice_map(
    proc: &Process,
    juice: &JuiceTarget,
    cane: &CaneTargets,
) -> Result<MapBackup, String> {
    let j = juice.nel_item;
    let c = cane.nel_item;
    let def_cane = cane.cane_item;

    if j == 0 || c == 0 || def_cane == 0 {
        return Err("地址无效，请先重新定位".into());
    }

    let cat = read_u32_or(proc, j + NEL_CATEGORY, "juice category")?;
    let fn_detail = read_u64_or(proc, j + NEL_FN_GET_DETAIL, "juice FnGetDetail")?;
    let key_ptr = read_u64_or(proc, j + NEL_KEY, "juice key")?;
    let itemdata = read_u64_or(proc, def_cane + CANE_ITEM_DATA, "DefaultCane.ItemData")?;

    let cane_fn = read_u64_or(proc, c + NEL_FN_GET_DETAIL, "cane FnGetDetail")?;
    if cane_fn == 0 {
        return Err("法杖 FnGetDetail 为空，无法复制委托".into());
    }

    let default_str = read_u64_or(proc, def_cane + CANE_KEY, "DefaultCane.key")?;
    if let Some(s) = read_mono_string(proc, default_str)
        && s != "default"
    {
        return Err(format!("DefaultCane.key 预期 default，实际 {s:?}"));
    }

    let new_cat = cat | CAT_CANE_BITS;
    let backup = MapBackup {
        juice: j,
        def_cane,
        cat,
        fn_detail,
        key_ptr,
        itemdata,
        applied: true,
    };

    write_u32_or(proc, j + NEL_CATEGORY, new_cat, "category")?;
    write_u64_or(proc, j + NEL_FN_GET_DETAIL, cane_fn, "FnGetDetail")?;
    write_u64_or(proc, j + NEL_KEY, default_str, "key→default")?;
    write_u64_or(proc, def_cane + CANE_ITEM_DATA, j, "CaneItem.ItemData")?;

    let check = new_cat & CAT_CANE_MASK;
    if check != CAT_CANE_BITS {
        log::warn(format!(
            "category 位域异常: new_cat={new_cat:#x} mask结果={check:#x}"
        ));
    }

    log::info(format!(
        "映射完成 juice={j:#x} def_cane={def_cane:#x} cat={cat:#x}→{new_cat:#x} fn={fn_detail:#x}→{cane_fn:#x} key={key_ptr:#x}→{default_str:#x} itemdata={itemdata:#x}→{j:#x}"
    ));
    Ok(backup)
}

pub fn restore_juice_map(proc: &Process, bak: &MapBackup) -> Result<(), String> {
    if !bak.applied || bak.juice == 0 {
        return Err("没有可恢复的映射备份".into());
    }
    write_u32_or(proc, bak.juice + NEL_CATEGORY, bak.cat, "restore category")?;
    write_u64_or(
        proc,
        bak.juice + NEL_FN_GET_DETAIL,
        bak.fn_detail,
        "restore FnGetDetail",
    )?;
    write_u64_or(proc, bak.juice + NEL_KEY, bak.key_ptr, "restore key")?;
    if bak.def_cane != 0 {
        write_u64_or(
            proc,
            bak.def_cane + CANE_ITEM_DATA,
            bak.itemdata,
            "restore ItemData",
        )?;
    }
    log::info(format!("映射已恢复 juice={:#x}", bak.juice));
    Ok(())
}

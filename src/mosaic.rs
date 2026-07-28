use crate::log;
use crate::mem::Process;
use crate::mono;

const PATCH: [u8; 3] = [0x31, 0xC0, 0xC3];

#[derive(Clone, Debug)]
pub struct MosaicPatch {
    pub addr: u64,
    pub orig: Option<[u8; 3]>,
    pub applied: bool,
}

impl MosaicPatch {
    pub fn empty() -> Self {
        Self {
            addr: 0,
            orig: None,
            applied: false,
        }
    }
}

pub fn apply(proc: &Process, prev: &MosaicPatch) -> Result<MosaicPatch, String> {
    if proc.is_alive() == Some(false) {
        return Err("游戏进程已退出".into());
    }
    let method = mono::find_method(
        proc,
        "Assembly-CSharp",
        "nel",
        "MosaicShower",
        "FnDrawMosaic",
        3,
    )?;
    log::info(format!("FnDrawMosaic method={method:#x}，开始同线程 JIT"));
    let addr = mono::compile_method(proc, method)?;
    if addr == 0 || !(0x10000..=0x0000_7FFF_FFFF_FFFF).contains(&addr) {
        return Err(format!("FnDrawMosaic 原生地址无效: {addr:#x}"));
    }
    if proc.is_alive() == Some(false) {
        return Err("写入补丁前游戏已退出".into());
    }

    let mut head = [0u8; 3];
    if !proc.read_bytes(addr, &mut head) {
        return Err(format!("读取 FnDrawMosaic 入口失败 addr={addr:#x}"));
    }

    if head == PATCH {
        let orig = if prev.addr == addr { prev.orig } else { None };
        log::info(format!("马赛克补丁已存在 addr={addr:#x}"));
        return Ok(MosaicPatch {
            addr,
            orig,
            applied: true,
        });
    }

    if !proc.write_code_bytes(addr, &PATCH) {
        return Err(format!("写入马赛克补丁失败 addr={addr:#x}"));
    }

    let mut verify = [0u8; 3];
    if !proc.read_bytes(addr, &mut verify) || verify != PATCH {
        return Err(format!("补丁校验失败 addr={addr:#x}"));
    }

    log::info(format!(
        "马赛克补丁已应用 addr={addr:#x} orig={:02X?}",
        head
    ));
    Ok(MosaicPatch {
        addr,
        orig: Some(head),
        applied: true,
    })
}

pub fn restore(proc: &Process, bak: &MosaicPatch) -> Result<(), String> {
    if !bak.applied || bak.addr == 0 {
        return Err("没有可恢复的马赛克补丁".into());
    }
    let Some(orig) = bak.orig else {
        return Err("无原始字节备份，请重启游戏".into());
    };
    if !proc.write_code_bytes(bak.addr, &orig) {
        return Err(format!("恢复马赛克失败 addr={:#x}", bak.addr));
    }
    log::info(format!("马赛克补丁已恢复 addr={:#x}", bak.addr));
    Ok(())
}

pub fn status(proc: &Process, bak: &MosaicPatch) -> String {
    if !bak.applied || bak.addr == 0 {
        return "关".into();
    }
    let mut cur = [0u8; 3];
    if !proc.read_bytes(bak.addr, &mut cur) {
        return format!("地址不可读 (addr={:#x})", bak.addr);
    }
    if cur == PATCH {
        format!("已禁用 (addr={:#x})", bak.addr)
    } else {
        format!("已恢复/未生效 (addr={:#x})", bak.addr)
    }
}

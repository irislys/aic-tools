use std::io::{self, Write};

use crate::find::{self, CaneTargets, JuiceTarget};
use crate::force_debug_menu::{self, F7Patch};
use crate::formula::{self, AXES};
use crate::game_version;
use crate::log;
use crate::map_ui::{self, MapBackup};
use crate::mem::Process;
use crate::mosaic::{self, MosaicPatch};

const PROC_NAME: &str = "AliceInCradle.exe";

pub struct Session {
    pub proc: Process,
    pub version: String,
    pub cane: Option<CaneTargets>,
    pub juice: Option<JuiceTarget>,
    pub map_bak: MapBackup,
    pub mosaic_bak: MosaicPatch,
    pub f7_bak: F7Patch,
}

impl Session {
    pub fn attach() -> Result<Self, String> {
        let proc = Process::open_by_name(PROC_NAME).map_err(|e| {
            let msg = format!("附加 {PROC_NAME} 失败: {e}");
            log::error(&msg);
            msg
        })?;
        let version = game_version::detect_content_version(&proc).map_err(|e| {
            let msg = format!("版本检测失败: {e}");
            log::error(&msg);
            msg
        })?;
        if !game_version::is_supported(&version) {
            let msg = format!(
                "版本 {version} 不在支持列表 {:?}",
                game_version::SUPPORTED_CONTENT_VERSIONS
            );
            log::error(&msg);
            return Err(msg);
        }
        log::info(format!("目标兼容性校验通过 pid={}", proc.pid()));
        Ok(Self {
            proc,
            version,
            cane: None,
            juice: None,
            map_bak: MapBackup::empty(),
            mosaic_bak: MosaicPatch::empty(),
            f7_bak: F7Patch::empty(),
        })
    }

    fn ensure_alive(&self) -> Result<(), String> {
        match self.proc.is_alive() {
            Some(true) => Ok(()),
            Some(false) => Err("游戏进程已退出".into()),
            None => Err("无法查询进程状态".into()),
        }
    }

    fn verify_cached(&self, addr: u64, label: &str) -> Result<(), String> {
        self.ensure_alive()?;
        if addr != 0 && !self.proc.looks_like_user_ptr(addr) {
            let msg = format!("{label} 地址 {addr:#x} 不再可读，请选 4 重新定位");
            log::warn(&msg);
            return Err(msg);
        }
        Ok(())
    }
}

fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn clear_screen() {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, COORD, FillConsoleOutputAttribute, FillConsoleOutputCharacterW,
        GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleCursorPosition,
    };

    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h == (-1isize as _) {
            println!();
            let _ = io::stdout().flush();
            return;
        }
        let mut info = std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>();
        if GetConsoleScreenBufferInfo(h, &mut info) == 0 {
            println!();
            let _ = io::stdout().flush();
            return;
        }
        let size = (info.dwSize.X as u32).saturating_mul(info.dwSize.Y as u32);
        let home = COORD { X: 0, Y: 0 };
        let mut written = 0u32;
        FillConsoleOutputCharacterW(h, b' ' as u16, size, home, &mut written);
        FillConsoleOutputAttribute(h, info.wAttributes, size, home, &mut written);
        SetConsoleCursorPosition(h, home);
    }
    let _ = io::stdout().flush();
}

fn print_menu(session: &Session) {
    let mos = mosaic::status(&session.proc, &session.mosaic_bak);
    let f7 = force_debug_menu::status(&session.proc, &session.f7_bak);
    println!("0.29j  pid={}", session.proc.pid());
    println!("1 法杖数值");
    println!("2 诺艾尔汁映射");
    println!("3 恢复映射");
    println!("4 重新定位");
    println!("5 禁用马赛克  [马赛克:{mos}]");
    println!("6 恢复马赛克");
    println!("7 启用F7调试  [F7:{f7}]");
    println!("0 退出");
    println!("提示: F7 须在标题界面启用；成功后游戏内按 F7；重启失效");
}

fn pause_enter(msg: &str) {
    let _ = read_line(msg);
}

pub fn run_menu(session: &mut Session) {
    loop {
        if let Err(e) = session.ensure_alive() {
            eprintln!("{e}");
            log::warn(&e);
            pause_enter("按回车键退出...");
            break;
        }
        clear_screen();
        print_menu(session);
        match read_line("> ").as_str() {
            "1" => {
                menu_cane_stats(session);
                pause_enter("按回车返回菜单...");
            }
            "2" => {
                menu_map_juice(session);
                pause_enter("按回车返回菜单...");
            }
            "3" => {
                menu_restore_map(session);
                pause_enter("按回车返回菜单...");
            }
            "4" => {
                session.cane = None;
                session.juice = None;
                log::info("清空定位缓存");
                println!("已清空定位缓存");
                pause_enter("按回车返回菜单...");
            }
            "5" => {
                menu_disable_mosaic(session);
                pause_enter("按回车返回菜单...");
            }
            "6" => {
                menu_restore_mosaic(session);
                pause_enter("按回车返回菜单...");
            }
            "7" => {
                menu_enable_f7(session);
                pause_enter("按回车返回菜单...");
            }
            "0" | "q" | "Q" => break,
            _ => {}
        }
    }
}

fn ensure_cane(session: &mut Session) -> Result<(), String> {
    if let Some(c) = &session.cane {
        session.verify_cached(c.nel_item, "NelItem")?;
        return Ok(());
    }
    let t = find::find_beginner_cane(&session.proc)?;
    session.cane = Some(t);
    Ok(())
}

fn ensure_juice(session: &mut Session) -> Result<(), String> {
    if let Some(j) = &session.juice {
        session.verify_cached(j.nel_item, "诺艾尔汁NelItem")?;
        return Ok(());
    }
    let t = find::find_noel_juice(&session.proc)?;
    session.juice = Some(t);
    Ok(())
}

fn menu_cane_stats(session: &mut Session) {
    if let Err(e) = ensure_cane(session) {
        eprintln!("{e}");
        return;
    }
    let cane = session.cane.as_ref().unwrap().clone();
    let pe = match cane.pr_equip {
        Some(p) => p,
        None => {
            eprintln!("未找到 PrCaneEquip，请装备初学者法杖后选 4 重新定位");
            return;
        }
    };

    if let Err(e) = session.verify_cached(pe, "PrCaneEquip") {
        eprintln!("{e}");
        session.cane = None;
        return;
    }

    let base = match formula::read_pr_fields(&session.proc, pe) {
        Some(f) => f,
        None => {
            eprintln!("读取 PrCaneEquip 失败，地址可能已失效");
            log::warn(format!("read_pr_fields 失败 pe={pe:#x}"));
            session.cane = None;
            return;
        }
    };

    if ![
        base.near_power,
        base.near_reach,
        base.mp_use,
        base.stability,
    ]
    .into_iter()
    .all(f32::is_finite)
    {
        eprintln!("读取到非有限值，地址可能已失效，请选 4 重新定位");
        log::warn(format!("PrCaneEquip 字段非有限 pe={pe:#x}"));
        session.cane = None;
        return;
    }

    print!("雷达: ");
    for a in AXES {
        print!(
            "{}={:.0} ",
            a.name_zh,
            formula::forward_display(a.id, &base)
        );
    }
    println!();

    let mut target = base;
    let mut selected = Vec::new();

    for axis in AXES {
        loop {
            let cur = formula::forward_display(axis.id, &target);
            let line = read_line(&format!(
                "{} {:.0} | {} | 目标: ",
                axis.name_zh, cur, axis.formula_hint
            ));
            if line.is_empty() {
                break;
            }
            let disp: f32 = match line.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("无效数字，请重新输入（回车跳过）");
                    continue;
                }
            };
            match formula::invert_display(axis.id, disp, &target) {
                Ok(next) => {
                    let back = formula::forward_display(axis.id, &next);
                    let note = if disp > 255.0 {
                        "（已按 UI 上限 255 写入）"
                    } else {
                        ""
                    };
                    eprintln!("  {} → 显示≈{:.1}{note}", axis.name_zh, back);
                    log::info(format!(
                        "axis={} display_in={disp} forward_back={back:.4}",
                        axis.name_zh
                    ));
                    target = next;
                    if !selected.contains(&axis.id) {
                        selected.push(axis.id);
                    }
                    break;
                }
                Err(e) => {
                    eprintln!("{e}，请重新输入（回车跳过）");
                }
            }
        }
    }

    if selected.is_empty() {
        return;
    }

    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }

    let mut written = Vec::new();
    for &axis_id in &selected {
        let axis = AXES.iter().find(|a| a.id == axis_id).unwrap();
        match formula::write_pr_axis(&session.proc, pe, axis_id, &target) {
            Ok(()) => written.push(axis.name_zh),
            Err(e) => {
                log::error(format!(
                    "写入 PrCaneEquip 失败 {} pe={pe:#x}: {e}",
                    axis.name_zh
                ));
                eprintln!("写入失败 {}: {e}", axis.name_zh);
            }
        }
        if let Err(e) =
            formula::write_cane_table_axis(&session.proc, cane.cane_item, axis_id, &target)
        {
            log::warn(format!(
                "CaneItem 表同步失败 {} cane={:#x}: {e}",
                axis.name_zh, cane.cane_item
            ));
        }
    }

    if written.is_empty() {
        eprintln!("全部写入失败，请检查进程状态");
    } else {
        log::info(format!(
            "已写入 {} 项: {}",
            written.len(),
            written.join(", ")
        ));
        println!("已写入 {} 项，重新打开法杖详情刷新雷达", written.len());
    }
}

fn menu_map_juice(session: &mut Session) {
    if let Err(e) = ensure_cane(session) {
        eprintln!("{e}");
        return;
    }
    if let Err(e) = ensure_juice(session) {
        eprintln!("{e}");
        return;
    }
    if session.map_bak.applied && !read_line("已有映射，覆盖？[y/N]: ").eq_ignore_ascii_case("y")
    {
        return;
    }
    let cane = session.cane.as_ref().unwrap().clone();
    let juice = session.juice.as_ref().unwrap().clone();

    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }

    match map_ui::apply_juice_map(&session.proc, &juice, &cane) {
        Ok(bak) => {
            session.map_bak = bak;
            println!("映射完成，重新打开诺艾尔汁详情查看雷达");
        }
        Err(e) => {
            log::error(format!("映射失败: {e}"));
            eprintln!("{e}");
        }
    }
}

fn menu_restore_map(session: &mut Session) {
    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }
    match map_ui::restore_juice_map(&session.proc, &session.map_bak) {
        Ok(()) => {
            session.map_bak.applied = false;
            println!("已恢复");
        }
        Err(e) => {
            log::error(format!("恢复失败: {e}"));
            eprintln!("{e}");
        }
    }
}

fn menu_disable_mosaic(session: &mut Session) {
    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }
    match mosaic::apply(&session.proc, &session.mosaic_bak) {
        Ok(bak) => {
            session.mosaic_bak = bak;
            println!("已禁用动态马赛克");
            println!("重启游戏会自动还原；本会话可用 6 恢复");
        }
        Err(e) => {
            log::error(format!("禁用马赛克失败: {e}"));
            eprintln!("{e}");
        }
    }
}

fn menu_restore_mosaic(session: &mut Session) {
    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }
    match mosaic::restore(&session.proc, &session.mosaic_bak) {
        Ok(()) => {
            session.mosaic_bak = MosaicPatch::empty();
            println!("马赛克已恢复");
        }
        Err(e) => {
            log::error(format!("恢复马赛克失败: {e}"));
            eprintln!("{e}");
        }
    }
}

fn menu_enable_f7(session: &mut Session) {
    if let Err(e) = session.ensure_alive() {
        eprintln!("{e}");
        return;
    }
    println!("扫描 initDebugger IL...");
    match force_debug_menu::apply(&session.proc, &session.f7_bak) {
        Ok(bak) => {
            session.f7_bak = bak;
            println!("F7 调试菜单已解锁");
        }
        Err(e) => {
            log::error(format!("启用 F7 失败: {e}"));
            eprintln!("{e}");
            eprintln!("提示: 回到标题界面后重试；确认版本为 0.29j");
        }
    }
}

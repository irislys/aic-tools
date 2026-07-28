mod aob;
mod find;
mod force_debug_menu;
mod formula;
mod game_version;
mod log;
mod map_ui;
mod mem;
mod mono;
mod mosaic;
mod offsets;
mod ui;

fn wait_enter() {
    use std::io::{self, Write};
    eprint!("按回车键退出...");
    let _ = io::stderr().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
}

fn main() {
    crate::log::init();
    crate::log::info("=== aic-tools 启动 ===");

    let mut session = match ui::Session::attach() {
        Ok(s) => {
            crate::log::info(format!("附加成功 pid={} 版本={}", s.proc.pid(), s.version));
            s
        }
        Err(e) => {
            crate::log::error(format!("附加失败: {e}"));
            eprintln!("附加失败: {e}");
            eprintln!("请先启动 AliceInCradle.exe（0.29j），并以管理员身份运行本工具。");
            wait_enter();
            std::process::exit(1);
        }
    };
    ui::run_menu(&mut session);
    crate::log::info("=== aic-tools 退出 ===");
}

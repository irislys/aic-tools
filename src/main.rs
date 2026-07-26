mod aob;
mod find;
mod formula;
mod game_version;
mod log;
mod map_ui;
mod mem;
mod mono;
mod mosaic;
mod offsets;
mod ui;

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
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    ui::run_menu(&mut session);
    crate::log::info("=== aic-tools 退出 ===");
}

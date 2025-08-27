use chrono::Local;
use env_logger::Builder;
use log::LevelFilter;
use std::io::Write;

fn logger(log_level_filter: LevelFilter) {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(Some("github_release_bot"), log_level_filter)
        .filter(None, LevelFilter::Info)
        .init();
}

fn debug_logger(log_level_filter: LevelFilter) {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}:{} - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.file().unwrap(),
                record.line().unwrap(),
                record.args()
            )
        })
        .filter(Some("github_release_bot"), log_level_filter)
        .filter(None, LevelFilter::Info)
        .init();
}

pub fn init_from_environment() {
    let debug_mode: bool = std::env::var("LOG_DEBUG")
        .unwrap_or("false".to_string())
        .parse()
        .unwrap();

    let log_level_filter: LevelFilter = std::env::var("LOG_LEVEL")
        .unwrap_or("info".to_string())
        .parse()
        .unwrap();

    if debug_mode {
        debug_logger(log_level_filter);
        log::debug!("Debug logger initialized");
    } else {
        logger(log_level_filter);
        log::info!("Logger initialized");
    }
}

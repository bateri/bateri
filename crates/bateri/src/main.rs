//! bateri — uygulama girişi.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    // Başsız ortam (SSH, CI): AppKit WindowServer'a bağlanamaz ve belirsiz
    // hata verir. Atlama ≠ geçme: 78 (EX_CONFIG) ile açıkça çık.
    if !aqua_oturumu() {
        // stdout: `kare=` satırıyla aynı kanal, `make duman` tek yerden okur.
        println!("ATLANDI: Aqua oturumu yok");
        return ExitCode::from(78);
    }
    // Bozuk değer sessizce "deadline yok"a dönmesin: duman kancası ya çalışır
    // ya kırmızı düşer.
    let run_seconds = match std::env::var("BT_RUN_SECONDS") {
        Ok(s) => match s.parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) => {
                eprintln!("bateri: BT_RUN_SECONDS sayı değil: {s:?}");
                return ExitCode::FAILURE;
            }
        },
        Err(_) => None,
    };
    match bt_shell::run(bt_shell::Options { run_seconds }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bateri: {e}");
            ExitCode::FAILURE
        }
    }
}

fn aqua_oturumu() -> bool {
    Command::new("launchctl")
        .arg("managername")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "Aqua")
        .unwrap_or(false)
}

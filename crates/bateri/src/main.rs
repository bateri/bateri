//! bateri — uygulama girişi.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    // Başsız ortam (SSH, CI): AppKit WindowServer'a bağlanamaz ve belirsiz
    // hata verir. Atlama ≠ geçme: 78 (EX_CONFIG) ile açıkça çık.
    if !has_aqua_session() {
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
    // `BT_SCROLL_TEST` yükü seçer, süreyi değil — süre `BT_RUN_SECONDS`'ta.
    // Yük istenip süre verilmezse koşu hiç bitmez: bu bir kullanım hatası,
    // sessizce sıfır saniyelik yüke düşmez.
    //
    // Env **yalnız burada** okunuyor ve kodun derinine tipli bir `Option`
    // olarak giriyor. Ayrıştırma yarısı `BT_RUN_SECONDS`'a benzemiyor ve
    // benzemesi de gerekmiyor: bu bayrağın **varlığı** anlam taşıyor, değeri
    // değil — `BT_SCROLL_TEST=0` da yükü seçer.
    let workload = match (std::env::var_os("BT_SCROLL_TEST").is_some(), run_seconds) {
        // Sıfır da eleniyor, yalnız eksik değişken değil: sıfır saniyelik bir
        // yük hiçbir şey ölçmez ve kapıya `glif=0` ile düşerek okuyanı var
        // olmayan bir boru hattı arızasına gönderirdi.
        (true, None | Some(0)) => {
            eprintln!("bateri: BT_SCROLL_TEST, sıfırdan büyük bir BT_RUN_SECONDS ister");
            return ExitCode::FAILURE;
        }
        (true, Some(_)) => Some(bt_shell::Workload::Load),
        // Geriye uyum: `BT_RUN_SECONDS` tek başına eskisi gibi duman yükü
        // seçiyor, yani `make duman` hiç değişmeden çalışır.
        (false, Some(_)) => Some(bt_shell::Workload::Smoke),
        (false, None) => None,
    };
    match bt_shell::run(bt_shell::Options {
        run_seconds,
        workload,
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bateri: {e}");
            ExitCode::FAILURE
        }
    }
}

fn has_aqua_session() -> bool {
    Command::new("launchctl")
        .arg("managername")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "Aqua")
        .unwrap_or(false)
}

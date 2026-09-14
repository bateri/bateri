//! bateri — uygulama girişi.

#[cfg(test)]
mod bundle_assets;

use std::process::{Command, ExitCode};
use std::time::Instant;

fn main() -> ExitCode {
    // **İlk satır.** Açılış damgası ne kadar erken alınırsa o kadar dürüst:
    // hemen altındaki `has_aqua_session()` bir alt süreç doğuruyor ve o da
    // bugün bateri'nin açılış yolunun bir parçası. Damga oradan sonra
    // alınsaydı `acilis=` o süreyi sessizce düşerdi.
    //
    // Yine de **süreç başlangıcı değil**: dyld ve Rust runtime kurulumu bu
    // damgadan önce bitmiş oluyor. Elimizdeki en erken nokta bu ve sayı
    // "main'den ilk tamamlanan kareye" demek.
    //
    // Kapı kapalıyken saat **hiç** okunmuyor (R4.1): `then` closure'ı yalnız
    // `true` üstünde koşar.
    let stats_since = std::env::var_os("BT_FRAME_STATS")
        .is_some()
        .then(Instant::now);
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
    // `BT_SCROLL_TEST` yükü seçer, `BT_FRAME_STATS` ölçümü açar; ikisi de
    // **süreyi** seçmez ve ikisi de süresiz anlamsızdır. Yük süresiz hiç
    // bitmez; ölçüm süresiz hiç raporlanmaz (rapor yalnız deadline yolunda
    // basılıyor) ve sessizce örnek biriktirip atardı. Sıfır da eleniyor:
    // sıfır saniyelik bir koşu hiçbir şey ölçmez ve kapıya `glif=0` ile
    // düşerek okuyanı var olmayan bir boru hattı arızasına gönderirdi.
    //
    // Env **yalnız burada** okunuyor ve kodun derinine tipli bir alan olarak
    // giriyor (R4.2). Ayrıştırma yarısı `BT_RUN_SECONDS`'a benzemiyor ve
    // benzemesi de gerekmiyor: bu iki bayrağın **varlığı** anlam taşıyor,
    // değeri değil — `BT_SCROLL_TEST=0` da yükü seçer.
    let scroll_test = std::env::var_os("BT_SCROLL_TEST").is_some();
    for (name, asked) in [
        ("BT_SCROLL_TEST", scroll_test),
        ("BT_FRAME_STATS", stats_since.is_some()),
    ] {
        if asked && !matches!(run_seconds, Some(n) if n > 0) {
            eprintln!("bateri: {name}, sıfırdan büyük bir BT_RUN_SECONDS ister");
            return ExitCode::FAILURE;
        }
    }
    // Geriye uyum: `BT_RUN_SECONDS` tek başına eskisi gibi duman yükü
    // seçiyor, yani `make duman` hiç değişmeden çalışır.
    let run = run_seconds.map(|seconds| bt_shell::Run {
        seconds,
        workload: if scroll_test {
            bt_shell::Workload::Load
        } else {
            bt_shell::Workload::Smoke
        },
        stats_since,
    });
    match bt_shell::run(bt_shell::Options { run }) {
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

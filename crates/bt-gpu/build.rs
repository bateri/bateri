//! shaders/*.metal → $OUT_DIR/default.metallib. Derleme reçetesi tek yerde
//! burasıdır; `make shader` yalnız `touch` + `cargo build -p bt-gpu` sarmalayıcısıdır.
//! Derleme zamanı olduğu için `panic`/`assert` serbesttir (PTY yolu değil).

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    // Dizin olarak izlenir: yeni eklenen .metal de yeniden derlemeyi tetikler.
    println!("cargo:rerun-if-changed=shaders");
    // Taban macOS sürümü .cargo/config.toml'dan gelir; rustc binary'yi, biz
    // shader'ları aynı sayıya bağlarız — ikisi ayrışırsa metallib çalışma
    // zamanında reddedilir, derlemede değil.
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let taban = env::var("MACOSX_DEPLOYMENT_TARGET")
        .expect("MACOSX_DEPLOYMENT_TARGET tanımsız; .cargo/config.toml [env] tek kaynak");
    // `[env]` tablosu `force` olmadan kabuk ortamına yenilir; metal3.1 macOS 14
    // ister ve derleyici bu çifti denetlemez — tabanı burada bağlarız.
    let major: u32 = taban
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or_else(|| panic!("MACOSX_DEPLOYMENT_TARGET okunamadı: {taban:?}"));
    assert!(
        major >= 14,
        "taban macOS {taban}: -std=metal3.1 en az 14.0 ister"
    );
    let version_min = format!("-mmacos-version-min={taban}");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    xcrun_kontrol();

    let mut airs = Vec::new();
    for entry in fs::read_dir("shaders").expect("shaders/ dizini okunamadı") {
        let path = entry.expect("dizin girdisi").path();
        if path.extension().is_some_and(|e| e == "metal") {
            let air = out
                .join(path.file_name().expect("dosya adı"))
                .with_extension("air");
            kos(Command::new("xcrun")
                .args(["-sdk", "macosx", "metal"])
                .args(["-std=metal3.1", &version_min, "-c"])
                .arg(&path)
                .arg("-o")
                .arg(&air));
            airs.push(air);
        }
    }
    assert!(!airs.is_empty(), "shaders/ altında .metal yok");
    // read_dir sırası tanımsız; metallib baytları deterministik kalsın.
    airs.sort();
    kos(Command::new("xcrun")
        .args(["-sdk", "macosx", "metallib"])
        .args(&airs)
        .arg("-o")
        .arg(out.join("default.metallib")));
}

fn xcrun_kontrol() {
    let ok = Command::new("xcrun")
        .args(["-sdk", "macosx", "-f", "metal"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        ok,
        "bateri: `xcrun -sdk macosx -f metal` başarısız. Metal shader derleyicisi \
         için Xcode gerekiyor; Command Line Tools tek başına `metal`'ı taşımaz. \
         `xcode-select -p` Xcode'u göstermeli."
    );
}

fn kos(cmd: &mut Command) {
    let durum = cmd
        .status()
        .unwrap_or_else(|e| panic!("{cmd:?} başlatılamadı: {e}"));
    assert!(durum.success(), "shader derlemesi başarısız: {cmd:?}");
}

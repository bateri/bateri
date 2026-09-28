# Phase 1 — `make linux`: bt-core'un Linux kapısı

## Özet

`bt-core`'u Docker'da Linux'ta derleyip sınayan `make linux` hedefini ve
imaj tarifini kurmak. Bu hedef `CLAUDE.md`'nin "kapı Linux hedefiyle
derlemedir" cümlesini bir komuta bağlıyor.

_Requirements: R1.1, R1.2_

## Değişiklikler

- **İmaj tarifi (yeni, depoda tek dosya; yer ve ad kodlayanın, ör. `tools/linux/Dockerfile`)**
  - Taban `rust:{yerel sürüm}-bookworm`.
  - `zsh` kurulu, `LANG=C.UTF-8`. Bu ikisinin gerekçesi `context.md` →
    Kanıt'taki üç ayna sınaması ve tarifin yorumunda tek cümleyle yazılı.
  - Sonraki setlerin eklemeleri (lavapipe, başsız Wayland) şimdi
    **eklenmiyor**.
- **`Makefile`**
  - Yeni `linux` hedefi, `.PHONY`'ye de giriyor.
  - Sıra: önce yerel `rustc --version`'ın major.minor'ünü imaj etiketiyle
    karşılaştırıyor; uyuşmazsa Türkçe tanıyla kırmızı düşüyor. Sonra imajı
    kuruyor (önbellekli).
  - Konteynerde çalışan komutlar: `cargo clippy -p bt-core --all-targets
    --locked -- -D warnings` ve `cargo test -p bt-core --locked`.
  - `CARGO_TARGET_DIR=target/linux`, crate önbelleği adlı bir volume, depo
    `/w`'ye bağlı.
  - Docker yoksa ya da daemon cevap vermiyorsa "koşamadı" diyor ve `make
    duman`'ın `ATLANDI` emsaliyle ayırt edilebilir bir çıkış kodu veriyor.
    Sürüm uyuşmazlığı bu kola düşmüyor.
  - Hedefin yorumu `make duman`'ınki gibi: ne sınanıyor, neden `make
    hepsi`'nin dışında, kapsamın setlerle nasıl büyüyeceği
    (`docs/YOL-HARITASI.md`'ye işaretçi).
- **`.claude/is-akisi/proje.md`**
  - Doğrulama tablosuna yeni satır: "Linux'ta derlenen bir crate (bugün
    `bt-core`) değiştiyse → `make linux`".
  - `[~]` yalnız Docker yokken yazılıyor. Sürüm uyuşmazlığı "koşamadı"
    değil.
  - Otonom şerit eklerinde uzun komutlar listesine `make linux` ekleniyor.
- **`CLAUDE.md`**
  - Komutlar bloğuna tek satır.
  - Katman tablosunda `bt-core` satırının "kapı Linux hedefiyle derlemedir"
    ifadesi `make linux`'a bağlanıyor.
  - Kural + tek cümle gerekçe; ölçüm anlatısı buraya değil `context.md`'ye
    ait.
- **`.gitignore`**
  - `target/` zaten dışarıdaysa değişiklik yok. `target/linux` onun altında
    kalıyor; bunu doğrula.

## Kabul

- `make linux` bu makinede yeşil: 602 geçen, 0 düşen `bt-core` sınaması ve
  temiz clippy (sayı `context.md`'deki elle koşuyla aynı ortam).
- Konteyner koşusundan sonra `git status` temiz: `Cargo.lock` değişmemiş,
  yeni izlenen dosya yok.
- `rustc` sürümü tarifteki etiketten farklıyken (sınamak için etiket geçici
  değiştirilir) hedef kırmızı düşüyor.
- `make hepsi` yeşil.

## Checklist

- [ ] İmaj tarifi (pin'li taban, zsh, `LANG`)
- [ ] `make linux` hedefi: sürüm eşleşmesi, `--locked`, `target/linux`, volume, Docker yokken ayırt edilir çıkış
- [ ] `proje.md` Doğrulama satırı ve uzun komut listesi
- [ ] `CLAUDE.md` Komutlar satırı ve `bt-core` kapısı cümlesi
- [ ] Test: `make linux` yeşil; sürüm uyuşmazlığında kırmızı; koşu sonrası `git status` temiz
- [ ] Doğrulama geçti (`make hepsi` + `make linux`)

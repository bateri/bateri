# Phase 4 — Üç stil ve `[motion] cursor_motion`

## Özet

Hareket stili ayar dosyasından seçilir: `snap`, `ease`, `spring` (varsayılan
`spring`). Kayıt anında uygulanır, süreli koşu okumaz.

_Requirements: R5 (cursor_motion yarısı), R3.1_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[motion]` bölümü tanınır ve
  `cursor_motion` üç değerli bir enum olarak ayrıştırılır (`Osc52` emsali:
  dizgi → enum, kabul edilmeyen değer kendi anahtarını değiştirmez ve bir
  `Diagnostic` bırakır). `osc52`'nin "kabul edilmeyen değer kapalıya düşer"
  istisnası buraya **geçmez**: yanlış tahminin bedeli görünür bir animasyon,
  sessiz bir pano sızıntısı değil. `Settings`'e alan, `Default`'a `spring`,
  `changes`'e fark. Modül başlığındaki "bilinmeyen anahtar örneği: `[motion]`"
  cümlesi artık yalan — başka bir ad verilir.
- **`crates/bt-gpu/src/motion.rs`** — üç stil: `snap` (anında; animasyon hiç
  başlamaz, yani hareket karesi de doğmaz), `ease` (sabit süre, taşma yok),
  `spring` (phase-3'ün fiziği). Süreler ve yay katsayıları **seçilmiş**
  sayılardır, ölçülmüş değil; tek yerde, doc'larıyla dururlar.
- **`crates/bt-gpu/src/link.rs`** — `DisplayLink` stili dışarıdan alır
  (`Renderer::set_font` emsali: `bt-shell` çözülmüş değeri verir). Stil
  değişimi uçuştaki bir animasyonu ışınlamaz: `snap`'e geçiş onu yerleştirir.
- **`crates/bt-shell/src/app.rs`** — açılışta `Settings`'ten okunur;
  `reload_settings` farkta uygular (temanın ve fontun izlediği yol). Süreli
  koşu `Inputs::Hermetic` olduğu için ayarı **görmez**; bu, `timed_run_does_
  not_see_the_user` sınamasının kapsamına girer.
- **`docs/AYARLAR.md`** — yeni `### [motion]` bölümü: anahtar, değerler,
  varsayılan, hata davranışı. **`CLAUDE.md`** → ayarlar maddesine anahtarın
  adı girer.

## Kabul

- `settings.toml`'a `cursor_motion = "ease"` yazıp kaydetmek açık pencerede
  stili değiştirir; `"snap"` hareketi kapatır (hareket karesi doğmaz,
  `hareket=0`).
- Tanınmayan değer (`cursor_motion = "sprong"`) yalnız o anahtarı
  varsayılanda bırakır, pencere alt başlığında tanı gösterir, dosyayı bozmaz;
  diğer anahtarlar uygulanır.
- `[motion]` bölümü olmayan dosya `spring` ile çalışır; bilinmeyen anahtar
  (`[motion] keypress = "pop"`) sessizce yoksayılır.
- `make duman` etkilenmez: hermetik koşu dosyayı okumuyor.

## Yayın Etkisi

**ayar şeması** — yeni anahtar `[motion] cursor_motion`, varsayılan `spring`;
eski anahtar yok, silinen anahtar yok, bilinmeyen anahtar korunur.
`docs/AYARLAR.md` aynı commit'te. shader yok · terminfo yok · tema yok · shell
entegrasyonu yok · app bundle yok · yeni bağımlılık yok.

Davranış değişikliği kullanıcıya görünür: `[motion]` yazılı bir dosya bugüne
kadar sessizce yoksayılıyordu, artık uygulanır (`plan.md` → Göç).

## Checklist

- [ ] `settings.rs`: `[motion] cursor_motion`, enum, varsayılan, `changes`,
      başlık yorumunun düzeltilmesi
- [ ] `motion.rs`: üç stil, seçilmiş sayıların doc'ları
- [ ] `link.rs`: stilin dışarıdan gelmesi, uçuştaki animasyonun akıbeti
- [ ] `app.rs`: açılış + canlı uygulama, hermetiklik
- [ ] `docs/AYARLAR.md` + `CLAUDE.md`
- [ ] Test: üç değerin ayrıştırılması, tanınmayan değerin tanısı, hermetik
      koşunun ayarı görmemesi
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
- [ ] Yayın etkisi yazıldı

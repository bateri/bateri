# Phase 5 — Prompt'un devri

## Özet

`PS1` ve `RPS1` sıfır görünür genişliğe insin, `>` dock'ta prompt'un yerini
alsın, blok çıpası `preexec`'e taşınsın ve kullanıcıya geri dönüş anahtarı
verilsin.

_Requirements: R4.1, R4.2, R4.3_

## Değişiklikler

- **`assets/shell/zsh/bateri.zsh`** — sıfır görünür genişlik.
  - **`RPS1`/`RPROMPT` de boşaltılır.** Bugün dosyada hiç geçmiyor; PS1 tek
    başına yetmez (011 Karar 12'nin zorunlu ayrıntısı).
  - **Dayatma aynanın ZLE kancasından yapılır**, `precmd`'den değil:
    p10k/starship `PS1`'i `precmd`'den **sonra** kendi ZLE kancalarından
    yeniden kuruyor ve aynı yerden dayatılmazsa tema kazanır.
  - **Çıpa `preexec`'e taşınır** (`anchor_close` → `preexec`). Alacritty
    kaynağında doğrulandı (011 Karar 10a): sıfır genişlikli PS1'de çıpa hayatta
    kalıyor — `zle reset-prompt`, Ctrl-L, geçmişte gezinme ve `TRANSIENT_PROMPT`
    kırmıyor. `Cell::set_underline_color(None)` `extra`'yı ancak hyperlink de
    yokken düşürüyor; yeniden çizilen hücre PS1'in şablonundan çıpayı geri
    alıyor.
- **`crates/bt-core/src/settings.rs`** — `prompt` anahtarı:
  `"terminal"` (varsayılan) ya da `"shell"`. Tanınmayan değer **yalnız bu
  anahtarı** etkiler ve uyarı görünür (`Settings::parse_keeping` örüntüsü);
  bilinmeyen anahtar **silinmez**.
  - `"shell"` prompt'u kullanıcıya geri verir **ama dock'u kapatmaz**: dock
    kabuğun prompt'unu değil ZLE'nin tamponunu çiziyor. İkisinin ayrı olması
    kasıtlı — kullanıcı prompt'unu geri almak için dock'tan vazgeçmek zorunda
    kalmamalı.
  - Kabuk çoktan doğduğu için **sonraki oturumda** geçerli (`shell.integration`
    ile aynı sınıf) — `docs/AYARLAR.md` bunu söyler.
- **`crates/bt-core/src/session.rs`** — `>`'in rengi safhadan çözülür ve dock
  kaydında sınırı geçer (koşuyor / başarılı / hatalı).

**Bu phase'ten önce dock çizilmiş olmak zorunda** (phase-3, phase-4). Ters sıra
promptsuz bir terminal bırakır ve `make hepsi`, `make duman`, `make kur`
**üçü de yeşil** kalır — prompt yolunu hiçbir kapı görmüyor.

## Kabul

- Kullanıcının prompt'u ızgarada görünmüyor; yerine dock'ta `>` var.
- `>` safhaya göre renkleniyor; çıkış kodu rengi 010'un yolundan geliyor.
- **Komut blokları yaşıyor:** çıpa `preexec`'ten geliyor ve şerit komutun
  satırında duruyor.
- p10k ya da starship kurulu bir oturumda tema prompt'u **geri yazamıyor**
  (dayatma ZLE kancasından).
- `prompt = "shell"` yazıp kaydeden kullanıcı sonraki oturumda kendi
  prompt'unu geri alıyor, dock **duruyor**.
- Tanınmayan `prompt` değeri yalnız bu anahtarı etkiliyor, uyarı görünüyor.

## Yayın Etkisi

- **Ayar şeması büyüyor:** `prompt` anahtarı, varsayılanı `"terminal"`,
  tanınmayan değer davranışı ve **sonraki oturumda geçerli** notu
  `docs/AYARLAR.md`'ye girer. Bilinmeyen anahtar asla silinmez.
- **Varsayılan yıkıcı:** kurulu kullanıcı prompt'unu kaybediyor. `AYARLAR.md`'nin
  kurtarma bölümüne satır eklenir; `shell.integration = "off"` da hâlâ bir
  çıkış ama **blokları da öldürüyor**, yani orantısız olan yol adıyla yazılır.
- **`make kur` zorunlu** (`assets/shell/*` değişti).
- **Shell entegrasyonu:** yalnız zsh; bash/fish'te prompt devri **yok**,
  `CLAUDE.md`'nin ilgili maddesi bunu söyler.
- **Bilinen sınır:** p10k **instant prompt** kancalarımızdan önce koşup ilk
  kareyi kendi prompt'uyla çiziyor.
- shader, terminfo, tema biçimi: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] `PS1` **ve** `RPS1`/`RPROMPT` sıfır görünür genişlikte
- [ ] Dayatma ZLE kancasından (p10k/starship geri yazamıyor)
- [ ] Çıpa `preexec`'e taşındı; bloklar ve şerit yaşıyor
- [ ] `prompt` anahtarı: varsayılan, tanınmayan değer, sonraki oturum notu
- [ ] `prompt = "shell"` dock'u **kapatmıyor**
- [ ] Test: çıpanın `preexec`'ten gelmesi; blok kimliği akışta
- [ ] Test: `prompt` anahtarının ayrıştırılması ve round-trip (bilinmeyen
      anahtar korunuyor)
- [ ] Gerçek zsh oturumunda gözle: p10k/starship kurulu, prompt devredilmiş
- [ ] `docs/AYARLAR.md` (anahtar + kurtarma satırı)
- [ ] Doğrulama geçti (`make hepsi` + `make kur`)
- [ ] Yayın etkisi yazıldı

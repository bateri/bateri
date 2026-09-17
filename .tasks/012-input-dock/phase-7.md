# Phase 7 — Alternatif ekranda dock kalkıyor

## Özet

vim, htop gibi alternatif ekrana geçen uygulamalar pencereyi **tamamen** geri
alsın; dock kalksın ve çıkışta insin.

_Requirements: R5.2, R5.3, R6.1_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — alternatif ekran bayrağı sınırı geçer.
  Bugün `Term` kilidi altında okunuyor ama `pub` değil; `Cursor`'a değil
  **kendi sorgusuna** eklenir (`Cursor` kare kaydı, bu bir oturum gerçeği).
- **`crates/bt-shell/src/app.rs`** — geçiş görülünce ızgara yüksekliği
  değişir ve `Session::resize` çağrılır.
  - **Resize render yolundan çağrılmaz.** Kare yolu bayrağı görür ama
    `Session::resize` `Term` kilidini alıyor ve o kilit okuyucunun ayrıştırma
    lease'inin arkasında bekleyebilir — "render yolu bloklanmaz" kuralı. Çağrı
    `dispatch2` ana kuyruğundan **bir sonraki turda** koşar; emsali
    `child_exit` ve OSC 52'nin pano işi.
  - **Bedel komut başına değil, geçiş başına.** `git log`, `man`, pager gibi
    her gün koşan komutlar resize **ödemiyor**; yalnız alternatif ekrana giren
    uygulamalar ödüyor ve orada zaten tam yeniden çizim oluyor.
  - Öteleme geçişte zaten snap'liyor (011'in yön kuralı), yani imleç ve içerik
    sıçraması **ek bir maliyet değil**.
- **Kapı kararı (R6.1).** Prompt ve dock yolunu bugün hiçbir kapı görmüyor:
  `smoke_shell` `/bin/sh`, `bateri.zsh`'i koşturan sınama phase-2'de doğdu ama
  gerçek pencere yok, `make kur` betiği `cmp`'liyor. Bu phase **seçimi yazılı
  yapar**:
  - ya **üçüncü bir yük** (`smoke_shell`/`load_shell` yanına, dock tanığı) —
    `smoke_shell`'e ikinci yük **eklenemez**, çünkü `hucre=8 glif=6 kural=15`
    sözleşmesinin tek sahibi o;
  - ya da **"prompt yolu kapısızdır"** diye açıkça kabul ve elle koşu +
    `/measure` ile tutulur.
  - Üçüncü yük seçilirse `IDLE_FRAME_LIMIT`/`QUIET_FLOOR` yeniden türetilir mi
    sorusu açılır ve türetme **ayrı commit**'le iner (`proje.md`).

## Kabul

- `vim` açınca dock kalkıyor ve vim pencerenin **tamamını** alıyor; çıkınca
  dock iniyor.
- `htop`, `less`, `man` aynı davranıyor.
- Geçişte içerik sıçramıyor (öteleme snap'liyor) ve vim'in ilk çizimi
  bozulmuyor.
- `git log`, `ls` gibi alternatif ekran kullanmayan komutlarda **hiç resize
  yok** — dock yerinde kalıyor.
- Resize ana kuyruktan koşuyor; render yolunda `Term` kilidi beklenmiyor
  (`make test-yaris` yeşil).
- Dock kapalıyken (entegrasyonsuz oturum) bu yol hiç çalışmıyor.

## Yayın Etkisi

- **Riskli phase:** paylaşılan duruma ve kilit sırasına dokunuyor →
  `make test-yaris` **ve** phase sonunda `/code-review`.
- **`CLAUDE.md`:** "Bugünkü hâl" dock'un alternatif ekranda kalktığını söyler.
- **Kapı sözleşmesi:** R6.1'in seçimi burada **yazılı** olur. Üçüncü yük
  seçilirse jeton **eklenerek** girer (`jeton silinmez, eklenir`) ve
  `IDLE_FRAME_LIMIT`/`QUIET_FLOOR` türetmesi ayrı commit.
- **`docs/OLCUMLER.md`:** yeni yük eklenirse `## Boşta kare`'nin bağlı girdi
  listesi büyür (bugün dört).
- shader, ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Checklist

- [ ] Alternatif ekran bayrağı sınırı geçiyor (kendi sorgusu, `Cursor` değil)
- [ ] Geçişte dock kalkıyor/iniyor; ızgara yüksekliği değişiyor
- [ ] `Session::resize` **ana kuyruktan**, render yolundan değil
- [ ] Alternatif ekran kullanmayan komutta resize yok
- [ ] Kapı kararı **yazılı** (üçüncü yük ya da "kapısızdır" kabulü)
- [ ] Test: geçişte resize bir kez, doğru yönde
- [ ] Test: entegrasyonsuz oturumda yol hiç çalışmıyor
- [ ] Gerçek pencerede gözle: vim, htop, less gir/çık
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı

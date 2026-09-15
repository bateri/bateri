# Phase 8 — OSC 52 yazma yönü

## Özet

Uzak uygulamanın OSC 52 ile panoya yazmasını ayar anahtarıyla aç: `Adapter`'da
düşen `ClipboardStore` olayını kilitsiz tek bir yuvadan genel panoya taşı.

_Requirements: R9, R5, R10_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[clipboard] osc52 = "off" |
  "copy"`, varsayılan `"copy"`. **Kapalıya düşer:** tanınmayan değer
  (`"paste"`, `false`, sayı) → `"off"` + tanı. Açılışta ayrıştırılamayan
  dosyada yükleyicinin döndüğü varsayılanlar `osc52`'yi `"off"` verir;
  canlı yenilemede ayrıştırılamayan dosya hiçbir şey uygulamaz (phase-4),
  kullanıcının son seçimi kalır. Fark fonksiyonu kapsar.
- **`crates/bt-core/src/session.rs`** —
  - Kendi OSC 52 tipi (`pub`, alacritty tipi sızmaz) `SessionOptions`'a ve
    terminal seçeneklerine girer; phase-4'ün `Config`'i tamamından kuran
    fonksiyonu onu `Osc52::{Disabled, OnlyCopy}`'ye çevirir.
  - `Adapter`: `ClipboardStore(Clipboard, metin)` → `Wake`'in yeni çağrısı;
    `ClipboardStore(Selection, …)` (`p`/`s`) yoksayılır — macOS'ta birincil
    seçim yok, genel panoya yazmak kullanıcının panosunu sessizce ezerdi.
    `ClipboardLoad` düşmeye devam eder (okuma yönü yok, `OnlyCopy` onu
    zaten üretmiyor).
  - `TestWake` (`:1739`) yeni çağrıyı uygular ve metni kaydeder.
- **`crates/bt-core/src/wake.rs`** — yeni çağrı: pano metni. Varsayılan gövde
  **yok** (uygulayan unutamasın). Doc: okuyucu thread'de ve `Term` kilidi
  tutulurken gelir; uygulayan bloklamaz, kilit almaz, `Session`'a girmez;
  kapanış sırasında gelen yazmanın kaybolması zararsız.
- **`crates/bt-shell/src/app.rs`** — `ShellWake`:
  - **Kilitsiz tek yuva, son yazma kazanır:** metin atomik takasla yuvaya
    konur; yuva boştu ise ana kuyruğa **tek** iş atılır (`child_exit`'in
    `exec_async` kalıbı), iş yuvayı boşaltıp `clipboard::copy`'ye verir.
    Durmadan OSC 52 basan bir uygulama ana kuyruğa sınırsız iş yığamaz.
  - Yuva mantığı AppKit'ten ayrık sınanabilir biçimde (pano parametre;
    `clipboard.rs`'in emsali).
  - Canlı `osc52` değişimi uygulayıcıdan terminal seçeneklerine gider.
- **`docs/AYARLAR.md`** — `[clipboard] osc52`, ne işe yaradığı (ssh'taki
  vim'in kopyası), okuma yönünün neden olmadığı, bozuk değerde kapalıya
  düşme.

## Kabul

- Ayrıştırma: `"copy"`, `"off"`, tanınmayan değer → `off` + tanı; açılışta
  ayrıştırılamayan dosya → `off`.
- `Config` kurucusu: `osc52` değişince `scrollback` korunur, tersi de
  (phase-4'ün sınaması iki alana genişler).
- `Adapter`: OSC 52 `c` dizisi `TestWake`'e metni ulaştırır; `p` dizisi
  ulaştırmaz; `osc52 = "off"` iken `c` dizisi de ulaştırmaz.
- Yuva: art arda çok sayıda metin → tek iş, son metin yazılır; dışarıdan
  verilen panoya (genel pano değil).
- `make test-yaris` iki profilde yeşil (okuyucu thread yolu).
- `make duman` jetonları değişmez.
- Göz: bateri içinde `printf` ile OSC 52 dizisi basmak panoya yazar; ssh
  üstünden vim'in (`clipboard` sağlayıcısı OSC 52) kopyası yerel panoya
  gelir; `osc52 = "off"` kaydedince aynı dizi panoya dokunmaz.

## Yayın Etkisi

- **ayar şeması** — `[clipboard] osc52`.
- Güvenlik davranışı: varsayılan açık (`copy`); arka plandaki uzak uygulama
  panoya yazabilir, `"off"` kapatır. Okuma yönü yok.
- `CLAUDE.md` katman tablosu `bt-core` satırı: OSC 52 artık panoya ulaşıyor
  (yol `Wake` üzerinden).

## Checklist

- [ ] `[clipboard] osc52`, kapalıya düşme, fark
- [ ] Kendi OSC 52 tipi; `Config` kurucusunda çeviri
- [ ] `Wake` yeni çağrısı (varsayılan gövdesiz), doc; `TestWake`
- [ ] `Adapter` kolu: `Clipboard` iletilir, `Selection` yoksayılır
- [ ] `ShellWake` atomik tek yuva → ana kuyruk → `clipboard::copy`
- [ ] Test: ayrıştırma, `Config` koruması, `Adapter` iletimi, yuva birleştirmesi
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [ ] Yayın etkisi yazıldı

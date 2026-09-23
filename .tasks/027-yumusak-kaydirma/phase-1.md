# Phase 1 — `bt-core`: kesirli kaydırma konumu

## Özet

`Session` kaydırmanın kesrini tutar ve delta çağrısıyla oynatır; tam satır
`scroll_locked`'ta kalır, `frame()` kesri ve tepe satırını sınırdan verir.
Kimse henüz kesirli delta göndermiyor, yani ekranda değişen bir şey yok.

_Requirements: R1.1, R1.2, R1.3, R1.4, R1.5_

## Değişiklikler

- **`crates/bt-core/src/session.rs`**
  - `Session`'a kesir (`[0, 1)`, atomik; kilit rejimi `fill_shown` emsali ya
    da `Term` kilidi altında — hangisi seçilirse doc'unda gerekçesi),
    kaydırma nesli ve süzülme isteğinin birikimi (işaretli satır, kare yolu
    alıp sıfırlar).
  - Delta gövdesi (adı uygulamada, `Term` kilidi altında tek yer): kesre
    ekler, taşan tam kısmı `scroll_locked`'a indirir (bant eşlemesi
    dokunulmadan), uçta kesri sıfırlar. **İki girişi var ve uyandırma
    yalnız birinde:** olay yolu (tekerleğin doğrudan kolu ve yerleşme) ofset
    ya da kesir değiştiyse kare ister (`wake_if_moved`'ın genişlemesi),
    değişmediyse istemez; kare yolunun süzülme payı ise **`frame()`'in
    argümanı** olarak gelir, aynı kilit turunda uygulanır ve **uyandırmaz** —
    animasyonun kare talebi `Waker::wake`'ten geçemez (`CLAUDE.md` → Boşta
    sıfır kare). Kesir sıfırken argüman `0` ve kare bugünküyle aynı.
  - Yerleşme niyeti (`bt-shell` yalnız niyeti gönderir, kesri bilmez): payı
    `round(kesir) − kesir` kilit altında hesaplanıp süzülme isteği olarak
    birikir; momentum başı niyeti birikmiş isteği düşürür ve nesli artırır —
    uçuştaki yerleşme de biter (kalan pay düşer; göreli model, sıçrama yok).
  - `scroll_wheel` imzası kesirli ve tam satır miktarı ile niyeti alır (tip
    `bt-core`'da, AppKit'siz). Rota bugünkü gibi önce; `Arrows`/`Report` tam
    satırla, `Scroll` niyete göre: tam satır (`off`), doğrudan delta ya da
    süzülme isteği biriktir + kare iste. Dönen `Wheel`'ın anlamı ve
    `bt-shell`'in artık sıfırlama kuralı kollara göre doc'ta.
  - `send_input`'un dibe dönüşü ve `scroll_page` kesri sıfırlar, nesli
    artırır ve birikmiş süzülme isteğini düşürür. Geçmişteyken gelen çıktı
    ve resize kesre dokunmaz.
  - `Cursor`'a kesir ve nesil. `frame()`: kesir `> 0` ise ızgaranın üstündeki
    satır (`offset == 0` iken bandın üstü) doldurma kanalından, bandın en
    üst (fill-yerel `0.`) satırı olarak; varlığı `Cursor::fill`'den **ayrı**
    bir alanda, yani `fill` bugünkü anlamında kalır ve çizen taraf kanalın
    boyunu ikisinin toplamı diye okur. Kapısı yalnız satırın
    defterde olması; `fill_rows`/`slide_fill_rows`'un dock, Ctrl-L ve
    `display_offset` kapıları uygulanmaz. `fill_shown`'a yazılmaz; bant
    döngüsünün "ofset terimi yok" doc'u yeni satır için güncellenir.
- **`crates/bt-shell/src/view.rs`** — yalnız yeni imzaya uyum: bugünkü tam
  satır miktarı ve `off` niyeti; davranış aynı.
- **`CLAUDE.md`** — `frame()` sınırının ve doldurmanın cümlelerine tepe
  satırı ve kesir (aynı commit'te, sözleşme kuralı).

## Kabul

- `every_notch_moves_the_screen_by_one_row_at_most` değişmeden yeşil.
- Yeni sınamalar: kesirli deltaların toplamı tam satırları bantlı ve
  bantsız pencerede sürekli üretir (ekran her adımda en çok bir satır
  kayar); dipte negatif ve tepede pozitif kesir kalmaz ve kare istenmez;
  geçmişteyken gelen çıktı kesri ve ofseti geri almaz; girdi ve Shift+PgUp
  kesri sıfırlayıp nesli artırır; tepe satırı `Line(-offset-1)` (bandlı dipte
  bandın üstü), Ctrl-L bayrağı kuruluyken de; kesir sıfırken tepe satırı
  yok ve kare bugünküyle aynı.
- `make hepsi` ve `make test-yaris` yeşil.

## Checklist

- [ ] Kesir, nesil, süzülme isteği ve delta çağrısı
- [ ] `scroll_wheel`'ın yeni imzası; `view.rs` uyumu
- [ ] Dibe dönüş ve Shift+PgUp kesri sıfırlıyor, nesil artıyor
- [ ] `Cursor` alanları ve `frame()`'in tepe satırı (ayrı sayı, ayrı kapı)
- [ ] Test: süreklilik, uçlar, dış yazıcılar, tepe satırı, kesirsiz kare aynı
- [ ] `CLAUDE.md` cümleleri
- [ ] Doğrulama geçti (`make hepsi`, `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi

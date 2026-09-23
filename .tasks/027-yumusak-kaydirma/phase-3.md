# Phase 3 — `bt-shell`: trackpad, momentum, çentik ve ayar

## Özet

`scrollWheel:` olayın niyetini sınıflayıp kesirli yolu açar,
`[motion] smooth_scroll` ayarı gelir ve belge yeni davranışa çevrilir —
özelliğin kullanıcıya göründüğü phase.

_Requirements: R3.1, R3.2, R3.3, R3.4_

## Değişiklikler

- **`crates/bt-shell/src/view.rs`**
  - Niyetin saf sınıflaması (`wheel_lines`'ın yanında, `NSEvent`'siz):
    girdiler hassas mı, `phase`, `momentumPhase`, delta ve birim; çıktı
    doğrudan kesirli delta / çentik süzülme isteği / yerleşme / momentum
    başında yerleşmeyi bitir / hiçbir şey. Yerleşmenin payını `bt-shell`
    hesaplamaz (kesri bilmiyor), niyeti gönderir. Yerleşme `phase == Ended` ve
    `momentumPhase == Ended`'da; momentum `Began`'ı yerleşmeyi bitiriyor
    (göreli model, sıçrama yok — `discussion.md` → Karar 3). Zamanlayıcı ve
    eşik yok.
  - `smooth` `bool`'u `false` iken bugünkü yol bayt bayt: `wheel_lines`,
    artık, sıfırlama kuralları. `true` iken kesirli miktar ve niyet; artığın
    sıfırlanması ok/rapor kolu için kalıyor. `follow_pointer` bugünkü
    koşulda (pencere kaydıysa).
- **`crates/bt-core/src/settings.rs`** — `[motion] smooth_scroll`
  (`"on" | "off"`, varsayılan `"on"`): ayrıştırma, tanı, `Settings::changes`,
  şablon, bilinmeyen anahtarı koruyan round-trip sınaması. Tip adı ve
  dizge-enum biçimi `CursorMotion` emsali.
- **`crates/bt-shell/src/window.rs`, `app.rs`** — ayar + Hareketi Azalt
  (çözülmüş) + `cursor_motion == Snap` tek `bool`'a iner (Hareketi Azalt'ın
  bugünkü birleşme yeri) ve view'a kayıt anında yayılır; `false`'a geçiş
  uçuştaki süzülmeyi bitirir (link'in `set_cursor_motion`/`set_reduce_motion`
  yolu zaten bitiriyor, `off` için aynı yol). Hermetik süreli koşu ayar
  okumuyor: varsayılan `"on"`.
- **`docs/AYARLAR.md`** — şablon, `[motion]` tablosu ve maddeleri:
  "Izgaranın başka sebeple yer değiştirmesi de kaymaz" maddesinden tekerlek
  çıkıyor; `smooth_scroll`'un anlamı (trackpad izler, çentik süzülür, jest
  sonunda satıra oturur; `off`, Hareketi Azalt ve `snap` satır adımı;
  alternatif ekran ve fare kipinde etkisiz; Mos gibi dış kaydırıcılar için
  kapatılabilir — `docs/ARASTIRMA.md`'ye atıfla, ölçülmüş iddia olarak
  değil).
- **`CLAUDE.md`** — "Tekerlek ve geometri ayrıca snap'ler" ve `bt-shell`'in
  tekerlek cümleleri; ayar listesine `smooth_scroll`.

## Kabul

- Niyet tablosu sınamaları: hassas/hassas olmayan, faz ve momentum
  dizileri (Began → Changed → Ended → momentum Began → … → momentum Ended),
  `smooth = false`'ta bugünkü `wheel_lines` çıktısı.
- Ayar sınamaları: varsayılan, tanınmayan değer, `changes`, round-trip.
- `make hepsi`, `make duman` yeşil (jetonlar değişmez).
- Gözle kontrol (set kapısının devir cümlesi, üç yüzey): ızgarada uzun bir
  çıktıda trackpad'le yavaş kaydırma parmağı piksel piksel izler, tepede
  yarım satır görünür ve bırakınca en yakın satıra oturur; fırlatmada
  momentum pürüzsüz yavaşlar; klasik tekerlek çentiği süzülür; doldurma
  bandı dipteyken ilk kaydırma bandın satırlarından sıçramadan devam eder;
  dock yerinde durur ve kayan ızgara onun altına girer; `smooth_scroll =
  "off"` ve Hareketi Azalt'ta satır adımı; vim/less ve Claude Code'da
  (fare kipi) tekerlek bugünkü gibi.

## Checklist

- [ ] Niyet sınıflaması (saf) ve `scrollWheel:`'ın iki kolu — momentum
  başı **ve** hassas jestin `phase == Began`'ı `ScrollIntent::GestureBegan`
  (phase-1: parmak yeniden değince uçuştaki yerleşme bitmeli)
- [ ] `smooth_scroll` ayarı: ayrıştırma, şablon, `changes`, uygulama
- [ ] Tek `bool` birleşmesi ve kayıt anında yayılım
- [ ] `docs/AYARLAR.md` ve `CLAUDE.md`
- [ ] Test: niyet tablosu, `off` kolunun bugünküyle aynılığı, ayar round-trip
- [ ] Doğrulama geçti (`make hepsi`, `make duman`) — `make duman` phase-2'nin `/code-review`
  düzeltmelerinden sonra ortam yüzünden koşulamadı (HEAD de kırmızıydı);
  burada yeşil görülmeli

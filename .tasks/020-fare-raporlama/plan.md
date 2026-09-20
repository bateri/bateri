# Fare raporlama

## Hedef

Fare isteyen uygulama fareyi alsın: tıklama, bırakma ve hareket rapor olarak
PTY'ye gitsin. Motive eden belirti — Claude Code'un giriş kutusunda tıklanan
yere imlecin gelmemesi — phase-1'de kapansın.

Kullanıcının metin seçme yeteneği **kaybolmasın**: Shift basılıyken fare
terminalin kalsın.

## Gereksinimler

- **R1** — Fare kipi (1000/1002/1003) açıkken Shift'siz basış ve bırakma
  uygulamaya rapor olarak gider; kodlaması kipten (1006 SGR / 1005 UTF-8 /
  X10) ve koordinatı uygulamanın ekranından.
- **R2** — **Shift tek kaçış yolu**: Shift basılıyken rapor gitmez, seçim
  başlar. Basış raporlandıysa jest uygulamanındır — sürükleme için ayrı kip
  kapısı yok.
  - **R2.1** — Asimetri bekçili: Shift düğmeyi geçersiz kılar, **tekerleği
    kılmaz** (`mouse_mode_comes_first_on_either_screen` değişmiyor).
  - **R2.2** — Shift rapora **hiç girmiyor**: arbitraj onu yuttuğu için
    değiştirici bitlerinde Shift (4) hiç kurulmaz; Meta (8) ve Control (16)
    kurulur.
- **R3** — Karar `bt-core`'da: `input::button_route` tablosu `wheel_route`'un
  yanında ve PTY'siz sınanıyor; `bt-shell` yalnız AppKit çevirisi. Kip
  dışarıdan sorulmuyor.
- **R4** — Cevap **üç varyantlı** (`Sent` / `Select` / `Ignored`), `Wheel`
  emsali. `bool` yasak: "kip kapalı" ile "rapor düştü" ayrı şeyler.
- **R5** — Rapor `send`'den geçer, `send_input`'tan **değil**: seçimi
  temizlemez, pencereyi dibe döndürmez.
- **R6** — Rota `mouseDown:`'da kilitlenir ve **bırakma basışı izler**:
  `Sent` basışın bırakması kırpılarak raporlanır, asla düşürülmez.
- **R7** — Hareket raporu (phase-2): 1002 yalnız basılıyken, 1003 her zaman;
  rapor **hücre değişiminde** kısılır ve kısma `bt-core` çağrısından önce.
- **R8** — Doldurma bandının üstündeki basış reddedilir (rapor da seçim de yok).
- **R9** — Kodla çelişen cümleler aynı commit'te düzelir: `input.rs` modül
  başlığı, `CLAUDE.md`'nin `bt-core` satırı.

## Yaklaşım

1. `input::mouse_report` — bugünkü `wheel_report` genelleşiyor: SGR'ın bırakma
   biçimi (`m`), X10'un bırakma düğmesi (3) ve değiştirici bitleri (Meta 8,
   Control 16) ekleniyor. Tekerlek aynı fonksiyondan geçmeye devam ediyor.
2. `input::mouse_encoding(mode)` — kodlama seçimi `wheel_route`'un içinden
   çıkıp paylaşılıyor; tablo tek yerde kalıyor.
3. `input::button_route(mode, shift)` — `wheel_route`'un kardeşi, aynı
   dosyada, aynı örüntüde.
4. `Session::mouse_button(button, pressed, at, shift)` — tek `Term` kilidi:
   kip sorusu, koordinat inişi (`viewport_point`) ve gönderim birlikte.
   Dönüş üç varyantlı enum.
5. `view.rs` — `mouseDown:`/`mouseUp:` ve sağ/orta tuşun dört selector'ı bu
   çağrıyı yapıyor; `dragging` yalnız `Select` kolunda kuruluyor.
6. Phase-2 — `setAcceptsMouseMovedEvents:`, `mouseMoved:`/`mouseDragged:`
   hareket yolu, `ViewIvars`'ta son raporlanan hücre.
7. Belgeler.

## Kapsam Dışı

- **`?1004` odak raporu** — setin kendi kapsam kuralı onu da dışarı atıyor
  (konusu fare değil odak), dosyaları ayrı (`app.rs`), ve `CLAUDE.md`'de
  adıyla yazılı bir mimari kararın düzeltilmesini ister. Kullanıcı odak
  belirtisi bildirmedi. Yol haritasına borç.
- **`?2031` tema değişimi bildirimi** — aynı ölçümde çıktı, konusu tema. Yol
  haritasına borç.
- **Yatay tekerlek raporu (66/67)** ve **SGR-pixel fare (1016)**.
- **Çift/üçlü tıkla kelime ve satır seçimi** — fare kipinden bağımsız.
- **Ayar anahtarı** — Shift kaçış yolu var; anahtar eklemek geri alınamaz.

## Akış

```
mouseDown: (sol/sağ/orta)
  │
  ├─ session_cell() → None  (doldurma bandı, sıfır grid) ──────→ hiçbir şey (R8)
  │
  └─ Session::mouse_button(button, pressed=true, at, shift)
       │  ← tek Term kilidi
       ├─ button_route(mode, shift)
       │    ├─ Select   (kip kapalı, ya da Shift basılı)
       │    └─ Report(encoding)
       │         ├─ viewport_point → satır uygulamanın ekranında mı
       │         │    ├─ hayır ────────────────────────────────→ Ignored
       │         │    └─ evet → mouse_report → send(Msg::Input) → Sent
       │         └─ (send_input DEĞİL: seçim durur, pencere dipte değil) (R5)
       │
       └─ view.rs
            ├─ Select  → dragging = true; set_selection(a, a)
            ├─ Sent    → rota kilitlendi (R6); dragging KURULMAZ
            └─ Ignored → hiçbir şey

mouseUp:
  ├─ rota Select ise  → dragging = false (bugünkü yol)
  └─ rota Sent ise    → bırakma raporu, satır KIRPILARAK (R6)
                         — düşürmek takılı düğme bırakırdı

mouseMoved: / mouseDragged:            (phase-2)
  ├─ hücre değişmedi ──────────────────→ dön (Term kilidi HİÇ alınmaz) (R7)
  └─ değişti → Session::mouse_motion(...) → 1002: yalnız basılıyken
                                            1003: her zaman
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| kapı | |

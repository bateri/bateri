# Fareyle seçim: ızgara ve dock

## Hedef

Izgarada ve dock'ta fareyle seçim bir macOS metin yüzeyinin beklentisini
karşılasın: çift tıklama kelime, üçlü tıklama satır, Shift+tıklama uzatır;
dock'ta tıklamak caret'i taşır, seçim silinir ya da üstüne yazılır; vurgu
temanın `selection` rengiyle, yuvarlak köşeli ve çok satırda tek parça.

## Gereksinimler

- **R1** — Izgara jestleri
  - **R1.1** — Çift tıklama kelimeyi, üçlü tıklama sarılmış mantıksal satırı
    bütün seçer; kelimenin tanımı `bt-core`'da tek sabit (`discussion.md` →
    Karar 5).
  - **R1.2** — Çift/üçlü tıklayıp sürüklemek kelime/satır adımıyla büyür.
  - **R1.3** — Shift+tıklama var olan seçimin ucunu taşır (fare kipinde de;
    orada seçimin yolu zaten Shift).
  - **R1.4** — Edit ▸ Select All (⌘A): dock caret'in sahibi değilken ızgaranın
    bütün geçmişini seçer. Cmd izin listesi değişmez.
  - **R1.5** — Jest defteri `NSEvent` görmeyen bir struct'ta ve sınanıyor.
- **R2** — Görünüş
  - **R2.1** — Temaya `selection` rolü: iki gömülü temada değeri, eksikte
    tabandan, `docs/AYARLAR.md`'de.
  - **R2.2** — Vurgu satır koşusu: ilk çizilir seçili hücreden sonuncusuna,
    aradaki boşluklar köprülü; çizilir hücresi olmayan satır koşu üretmez
    (Karar 4).
  - **R2.3** — Seçili metin hücrenin kendi ön planıyla, ters video çözülmüş
    (Karar 3).
  - **R2.4** — Odaksız pencerede seçim soluk (`dim_toward`; Karar 9).
  - **R2.5** — Koşular yuvarlak köşeli tek parça şekil: açıkta kalan köşe
    dışbükey yuvarlak, basamakta içbükey köşe; yarıçap
    `caret_radius_px(cell_px, CURSOR_RADIUS)`.
  - **R2.6** — Boşta sıfır kare korunur; `make duman`'ın jetonları seçimsiz
    koşuda oynamaz; seçim listesi sayaçlardan muaf.
- **R3** — Dock seçimi (terminal tarafı)
  - **R3.1** — Dock'un giriş satırında sürükleme, çift tıklama (kelime,
    ızgaranın davranışıyla aynı) ve üçlü tıklama (bütün `BUFFER`) seçer;
    yalnız `BUFFER`'ın karakterleri seçilir.
  - **R3.2** — Dock seçimi ızgarayla aynı görünüşte.
  - **R3.3** — Pencere başına tek seçim; ⌘C sahibin metnini kopyalar; ⌘A dock
    caret'in sahibiyken bütün `BUFFER`'ı seçer.
  - **R3.4** — `BUFFER` değişince ya da girdi gönderilince (`send_input`) dock
    seçimi kalkar.
  - **R3.5** — İsabet testi çizilen aynaya karşı ve dock'un tek sütun
    yürüyüşünden.
- **R4** — Dock düzenleme (kabukla)
  - **R4.1** — Sürüklemesiz tıklama caret'i tıklanan karaktere taşır.
  - **R4.2** — Seçim varken ⌫ ve ⌦ seçimi siler; yazılan ve yapıştırılan metin
    seçimin yerine geçer; ⌘X keser (Cut, yalnız dock seçimi ve kapı açıkken
    etkin).
  - **R4.3** — ← / → seçimi başına / sonuna daraltır; ⇧← / ⇧→ seçimi büyütür
    ya da caret'ten başlatır; başka her tuş seçimi kaldırıp bugünkü yolundan
    gider (Karar 8).
  - **R4.4** — Düzenleme kapısı dört koşul (dock sahibi, ekleme keymap'i,
    ayna güncel nesle cevap, yetenek bu prompt'ta görüldü); kapalıyken ZLE'ye
    hiçbir komut gitmez.
  - **R4.5** — Sarmalayıcı widget'ı ve bağlaması yalnız dock'lu kademede,
    `assets/shell/` altında; kullanıcının rc dosyasına yazılmaz; `make kur`
    yeşil.

## Yaklaşım

1. **Izgara jestleri** `bt-core` ile `bt-shell`'de, görünüş bugünkü ters
   videoda kalarak: `SelectKind`, kelime sabiti `term_config`'e, jest defteri
   struct'a, Select All menüye.
2. **Rol ve düz koşular**: `frame()` ters çevirmeyi bırakıp koşuları verir,
   `bt-gpu` onları `cell_bg` dörtgeni olarak zeminle glyph'ler arasına çizer.
3. **Yuvarlak şekil**: aynı koşular kendi fragment'inde, köşe maskesiyle.
4. **Dock seçimi** terminal tarafında: tek sütun yürüyüşü, çizilen aynanın
   izi, `DockSelection`, `Dock::selection`, tek sahip.
5. **Dock düzenleme**: betiğin widget'ı + yetenek + tek komut, tuş tablosu,
   Cut, tıkla-caret.

Gerekçeler `discussion.md` → `## Karar` ve `## Muhakeme`.

## Kapsam Dışı

- Doldurma bandının seçilebilmesi (`docs/YOL-HARITASI.md` → "Doldurma
  bandının satırları seçilemiyor").
- Izgarada tıklayarak ZLE caret'ini taşımak; dock'un bağlam satırında seçim.
- Kelime ayırıcı ayarı, seçim animasyonu, dörtlü tıklama (akıllı seçim).
- bash/fish widget'ı (betikleri yok).
- Çok satırlı `BUFFER`'da seçim: dock onu zaten göstermiyor (`Multiline`).

## Akış

```
fare (bt-shell view.rs)
  │  clickCount, Shift, konum ──► Gesture (saf struct) ──► hedef?
  │                                              ├─ ızgara ─► Session::set_selection(SelectKind) / extend
  │                                              └─ dock ───► Session::dock_select(…)  (çizilen aynanın izine karşı)
  ▼
Session::frame(.., runs: &mut SelectionRuns)   Session::dock(..) → Dock { selection, .. }
  │  satır koşuları (ilk..son çizilir seçili hücre)
  ▼
bt-gpu: zemin ─► seçim (köşe maskeli SDF, odak → renk) ─► glyph'ler ─► caret
                 (ızgara listesi ve dock listesi, kendi viewport'larında)

tuş / menü (dock seçimi varken, kapı açık)
  ⌫ ⌦ ⌘X ─► Session: send_input("\e[8133~d;S;E;L\a")
  yazma / ⌘V ─► d + olağan write / paste
  ←/→ ─► d;N;N;L (caret taşı)        ⇧←/⇧→ ─► yalnız terminalde
                                      │
                                      ▼
            zsh: __bateri_dock_edit (read -k … BEL; L tutmazsa no-op)
            line-init: bindkey main/emacs/viins + 8133;w
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | |
| phase-5 | |
| kapı | |

# Çok satırlı dock

## Hedef

Çok satırlı giriş — yapıştırma, geçmişten çok satırlı komut, `Esc-Enter`,
`for`/heredoc/`\`-devam — dock'ta kalsın: dock yukarı doğru büyür, satırlar
ve caret dock'ta durur, 030'un efektleri ve 031'in seçim/düzenlemesi orada
da çalışır. PTY'nin boyu değişmez; `DockStatus::Multiline` kalkar.

## Gereksinimler

- **R1** — Düzen
  - **R1.1** — `bt-core`'da tek düzen fonksiyonu: görüntüyü satır sonlarında
    böler, verilen genişlikte sarar (geniş karakter yarılanmaz), iki başlangıç
    alanı alır (ilk satır, devam satırları). Dock çizimi, isabet testi,
    efekt eşlemesi ve bastırma aynı fonksiyondan okur.
  - **R1.2** — Dock uzun tek satırlık komutu da sarar; `window_skip` emekli.
  - **R1.3** — Giriş satırları tavanı `DOCK_MAX_SHARE` (ızgara satırlarının
    yarısı, `bt-gpu`'da tasarım sabiti); aşınca caret'i izleyen durumsuz
    dikey pencere.
- **R2** — Bant
  - **R2.1** — PTY payı `DOCK_ROWS`'la sabit; çizilen bant `n` giriş satırı
    + bağlam satırı, boşluk yalnız giriş bloğu ile bağlam satırı arasında,
    saç çizgisi tepede ve o boşlukta. İki kavramın adı ayrık (ayrılan /
    çizilen).
  - **R2.2** — Izgara bandın o anki ek yüksekliği kadar yukarı çizilir; dolu
    ızgaranın tepesi kırpılır, giriş bitince döner. Izgaranın alt kenarı,
    doldurma bandı ve dock bandının üst kenarı aynı karede çakışır.
  - **R2.3** — Bandın ek satırı `Motion`'da kendi animatöründe, iki yönde
    süzülür, `settled()`'e girer; Hareketi Azalt, `cursor_motion = "snap"`
    ve geometri değişimi snap'ler. Bant değişen karede içerik ötelemesinin
    yükselen hedefi de süzülür. Boşta sıfır kare korunur.
  - **R2.4** — Fare: dock'un isabet testi çizilen bandın yayınlanmış
    geometrisinden (`Drawn`), ızgaranınki çizilen orijinden okur.
- **R3** — Çok satırın sahipliği
  - **R3.1** — `Multiline` kalkar; satır sonlu görüntü `Live`, satır ve caret
    dock'ta; `Control`, `Unavailable` ve alternatif ekran değişmez.
  - **R3.2** — Bastırma ızgaradaki bütün giriş satırlarını kapsar (imlecin
    altındakiler dahil); `PREBUFFER` varken üst taban çıpanın satırı.
  - **R3.3** — `PREBUFFER` aynanın yedinci, isteğe bağlı gövdesi (bütçeye
    girer); dock onu düzenlenebilir satırların üstünde çizer, seçilebilir,
    salt okunur. `PS2`'ye dokunulmaz.
  - **R3.4** — `PS2` satırları arasındaki `line-finish`, safha `Input`'ta
    kaldıkça görüntüyü, bandı ve bastırmayı `HANDOVER_HOLD` kadar tutar.
  - **R3.5** — `last_ink` `\n`'i saymaz; `blank_mirror` "görüntünün hiç
    karakteri yok" olarak yeniden tanımlanır.
- **R4** — Seçim ve düzenleme
  - **R4.1** — 2B isabet testi; sürükleme satırlar arası; vurgu görsel satır
    başına koşu; üçlü tıklama mantıksal satır, ⌘A bütün `BUFFER`.
  - **R4.2** — Tıkla-caret ve `d;S;E;L` çok satırda; `PREBUFFER`'a değen
    seçimde düzenleme tuşları seçimi kaldırıp bugünkü yoldan gider.
- **R5** — Satır sonlu yapıştırmanın arkasına, 031'in tam düzenleme kapısı
  yapıştırmadan önce açıksa, aynı yazımda `CSI 8133 ~ r BEL`; sonucu gerçek
  pencerede ölçülür.
- **R6** — 030'un efektleri (satır, sütun) konumunda; sarmayla satır
  değiştiren kayma iki eksende; eşlenemeyen hâl `Reset`.

## Yaklaşım

1. **Düzen ve ayna, görünmez** — `dock::layout`; bastırmanın `to`/`floor`'u
   ona taşınır (tek satırda bugünkü sonuç); ayna `PREBUFFER`'ı taşır ve
   çözülür ama çizilmez. `Multiline` duruyor.
2. **Değişken bant, görünmez** — `bt-gpu`/`bt-shell`: bandın çizilen satır
   sayısı, dipten yerleşim, bandın animatörü, `set_origin`'de birleştirme,
   `Drawn`'ın dock geometrisi, `Cursor`'dan gelen giriş satırı sayısı (bu
   phase'de hep 1).
3. **Sarma, görünür** — dock tek satırlık uzun komutu sarar ve büyür; 2B
   isabet, seçim koşuları, caret, tavan ve dikey pencere; çok satırda
   efektler geçici olarak `Reset`.
4. **Dönüşüm** — `Multiline` kalkar, satır sonlu görüntü ve `PREBUFFER`
   dock'ta, bastırma bütün satırlarda, `line-finish` tutması, sessiz
   kırılmaların bekçileri, `CLAUDE.md`.
5. **Yapıştırmanın tazelemesi** — `r` komutu ve ölçümü.
6. **Efektler iki eksende.**

Gerekçeler `discussion.md` → `## Karar` ve `## Muhakeme`.

## Kapsam Dışı

- `PS2`'nin dayatılması ya da dock'ta `%_` bağlamının (`for>`) çizilmesi.
- `PREBUFFER`'ın düzenlenmesi (ZLE onu kabul etmiş).
- Bağlam satırının çok satırı ya da sarması (bugünkü soldan kısaltma kalır).
- Çok satırlı yükün `can_be_typed` istisnasına girmesi (satır sonu komutu
  koşturur).
- bash/fish (betikleri yok; dock ZLE'nin aynasına bağlı).
- Doldurma bandının seçilebilmesi; kırpılan ızgara satırlarına tıklama.
- Yeni duman jetonu.

## Akış

```
zsh (ZLE)  u;CURSOR;PRE;BUF;POST;HL;KEYMAP;PREBUFFER   e   (w)
   │                                                         ▲ CSI 8133~ r (R5)
   ▼                                                         │
bt-core  DockState ──► frame(budget, …) ── dock::layout ──┬─► bastırma to/floor
                          │  (tek okuma)                  └─► Cursor::dock_rows
                          ▼
                     Session::dock(dock_rows, …) ─► hücreler (satır, sütun), dipten
   │
bt-gpu   Motion: origin(u16 hedef)  band(Slide, iki yön)  caret
         set_origin: çizilen = origin − band  ──► ızgara + doldurma viewport'u
         Frame: bant yüksekliği encode anında ─► dock viewport'u (dibe yaslı)
         publish: Drawn { px, fill_rows, dock }
   │
bt-shell window_point_cell (px)   window_point_dock (Drawn.dock, rows n)
         PTY: split_into_grid(DOCK_ROWS) — değişmez
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| phase-6 | ✅ |
| kapı | ✅ |

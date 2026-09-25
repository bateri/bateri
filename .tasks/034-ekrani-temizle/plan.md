# Ekranı temizle (⌘K) ve standart menü kalanları

## Hedef

⌘K ekranı ve geçmişi Terminal.app'in Clear to Start'ı gibi siler: o anki
prompt bloğu (ya da koşan komutun imleç satırı) kalır, yukarı kaydıracak
hiçbir şey kalmaz, kabuğa tek bayt gitmez. Aynı mekanizmanın yarısı ⌥⌘K
Clear Scrollback; yanında Terminal.app'in tek menü öğesiyle kapanan
kalanları — ⌘Home/⌘End/⌘PgUp/⌘PgDn ve ⌃⌘V Paste Escaped Text. Envanterin
kendi tasarımını isteyen yarısı yol haritasında (`context.md` → Envanter).

## Gereksinimler

- **R1 — Temizleme çekirdeği (`bt-core`).** Tek `Session` yöntemi, tek
  `Term` kilidi turu, iki kip: *başlangıca kadar* (⌘K) ve *yalnız geçmiş*
  (⌥⌘K).
  - **R1.1** — Korunan ilk satır: imlecin satırındaki blok kimliğini taşıyan
    bitişik satırların en üstü (sarılan ve çok satırlı giriş, `PREBUFFER`,
    çok satırlı `PS1` bütünüyle kalır); kimlik yoksa imlecin satırı. Bloğun
    başı ekranın üstündeyse hiçbir satır dışarı kaydırılmaz. Yalnız geçmiş
    kipinde dışarı kaydırılan satır sıfır.
  - **R1.2** — Pencere önce dibe (`Scroll::Bottom` + `reset_scroll`); korunan
    ilk satırın üstü ekranın tepesinden atılır, imleç ve `saved_cursor` aynı
    miktarda düşer, geçmiş silinir; ızgara ve dock seçimi kalkar.
  - **R1.3** — Kabuğa ve koşan programa bayt gitmez; komut koşarken de
    çalışır. Alternatif ekranda hiçbir şey yapmaz ve bunu dönüşüyle söyler.
  - **R1.4** — Temizleme `2J` neslinin ikinci yazarıdır (adlı tek yöntem,
    kilit altında): ardından doldurma bandı boş, kayma sayısı sıfır, son
    `2J` kalıntısı yok; ilk yeni geçmiş satırından sonra doldurma yalnız
    temizlemeden sonra gelen satırları verir.
  - **R1.5** — Arama: defter nesli artar, geçerli eşleşme kaybolur (geçmiş
    0 → 0 kolu dahil), sayım baştan başlar (`search_changed`); kare istenir.
- **R2 — Menü (`bt-shell`).**
  - **R2.1** — Edit ▸ Clear to Start ⌘K, Clear Scrollback ⌥⌘K; View ▸ Scroll
    to Top ⌘Home, Scroll to Bottom ⌘End, Page Up ⌘PgUp, Page Down ⌘PgDn —
    karşılayan `TerminalWindow`; kaydırma `scroll_page` yolundan (Top/Bottom
    `±i32::MAX` sayfa). Alternatif ekranda altısı da gri.
  - **R2.2** — Edit ▸ Paste Escaped Text ⌃⌘V (`BateriView`): satır sonu
    taşımayan metin `quote::shell_quote`'tan, satır sonu taşıyan metin
    bütünüyle tek tırnakla (`'` → `'\''`); ikisi de `Session::paste`'ten.
    Panoda metin yoksa öğe gri.
  - **R2.3** — `keyDown:`'ın Cmd izin listesi, Home/End'in yutulması ve
    `bt_core::Arrow`'un değişmezi değişmez.
- **R3 — Belge.** `CLAUDE.md` bugünkü sözleşmeye kural + gerekçe +
  işaretçiyle; `screen_clears`'ın ve "Dördüncü kol"un iki yazarlı hâli.

## Yaklaşım

1. `bt-core` (phase-1): `Session`'a temizleme yöntemi ve korunan ilk satırın
   yürüyüşü; `screen_clears`'ı artıran adlı yöntem ve doc'u; aramanın geçerli
   eşleşmesini düşüren kol. Çağıran yok, sınamalar hermetik oturumla.
2. `bt-shell` + belge (phase-2): altı menü öğesi ve `validateMenuItem:`
   kolları `TerminalWindow`'da, Paste Escaped Text `BateriView`'da;
   `CLAUDE.md` ve `docs/YOL-HARITASI.md`'nin 034 satırının kapanış notu.

Gerekçe `discussion.md` → Karar ve Muhakeme.

## Kapsam Dışı

- Alternatif ekranda birincil geçmişi silmek (`Term::inactive_grid` özel;
  Karar 2).
- Komut işaretleri üstünde gezinme (⌘↑/⌘↓, Select Between Marks, ⌘L, son
  çıktıyı kopyala), tıklanabilir bağlantılar, zil, Reset — yol haritası.
- Kabuğa ^L ya da dock komut kanalına yeni alt komut; `assets/shell/`
  değişmiyor.
- Home/End tuş kodlaması (yol haritasındaki "Klavye kalanları").
- Birincil ekranda göreli hareketle çizen bir programın (ink) ⌘K'den sonraki
  ilk karesi — her terminalde aynı, ^L onarır.

## Akış

```
⌘K / ⌥⌘K (menü) ── TerminalWindow ── alt ekran? → gri, çağrı yok
                                  └─ Session::clear_* ─┐
   Term kilidi: Scroll::Bottom + reset_scroll          │
                korunan ilk satır (blok yürüyüşü)       │  ⌥⌘K: 0 satır
                grid.scroll_up(k), imleç/DECSC −k       │
                clear_screen(Saved), seçimler ×         │
                screen_clears += 1 (adlı yöntem)        │
                defter nesli +1, geçerli eşleşme ×      │
   kilit sonrası: search_changed, request_frame ───────┘
frame(): nesil → bayrak, damga 0, fill 0, scrolled 0, clear_boundary 0
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| phase-2 | |
| kapı | |

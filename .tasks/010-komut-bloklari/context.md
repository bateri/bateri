# Komut blokları — Bağlam

## Mevcut Durum

009 kabuğun bastığı OSC 133 işaretlerini akışın içinden çekiyor ama **hiç
tüketmiyor**. Bugünkü zincir:

- `assets/shell/zsh/bateri.zsh` `precmd`/`preexec` kancalarından `A`, `B`,
  `C`, `D;{kod}` basıyor; `bt-shell` sarmalayıcıyı `ZDOTDIR` ile kuruyor.
- `bt-core::shell::Scanner` baytları PTY okuma yolunda (`TappedPty::read`)
  tarıyor, `Mark`'a çeviriyor; `ShellState::apply` onu yaprak kilitteki
  `Option<ShellState>` yuvasına işliyor.
- `Session::shell_state()` o durumu kopyalayarak veriyor: `phase`
  (`Prompt`/`Input`/`Running`/`Finished`) ve `last_exit`.

Tüketici yok: `bt-gpu` bu sorguyu hiç sormuyor, `frame()` sınırı komut
hakkında hiçbir şey taşımıyor. `teslim.md`'nin cümlesiyle: "ürün yüzeyi
**yok** — blok da dock da".

Çizim tarafında iki pipeline var (`cell_bg`, `cell`) ve `cell_bg`'nin
`Instance`'ı **genel bir piksel dörtgeni**: `pos`, `size`, `rgba`. Hücre
ızgarasına bağlı değil.

## Motivasyon

Komut bloğu Metalterm'i ekranda tanıtan ürün yüzeylerinden biri
(`docs/ARASTIRMA.md` → Ürün özellikleri: "komut blokları (süre, kırmızı
gutter)"). Kullanıcıya verdiği şey, bir ekran dolusu çıktıda gözün
aradığını bulması: komut nerede başladı, çıktısı nerede bitti, hangisi
başarısız oldu.

Yol haritasındaki yeri de bu: 009'un topladığı durum ilk kez burada ürüne
dönüşüyor ve 011'in (Input Dock) "blokların çalışıyor olması" varsayımı
buradan doğuyor.

## Kanıt

**Çıpa sorusu 009'da açıkça bu sete bırakıldı.** `discussion.md` → Karar 1:
"'prompt hangi satırda başladı' sorusu **yaklaşık** oluyor… bloklar setine
'çıpa kesin mi' sinyali ölçülebilir bir veri olarak geçsin". `plan.md` →
Kapsam dışı: "komut işaretlerinin **satıra çıpalanması** (bloklar setinin
işi)".

009 bu soruyu **yakalama** (capture) sorusu olarak çerçeveledi. Kodun
okunması ikinci ve daha ağır bir yarısını gösteriyor: **dayanıklılık**.
Doğrulanmış olgular (`alacritty_terminal 0.26`, kayıt kopyası):

1. **Yakalama yaklaşık.** `event_loop.rs:120-155` (`pty_read`) birden çok
   `read()`'i tek bir `advance()`'te işleyebiliyor: `Term` kilidi
   alınamazsa (`None => continue`) döngü okumaya devam ediyor ve baytlar
   birikiyor. Yani işaretin görüldüğü an ile ızgaranın o noktaya geldiği an
   aynı değil.
2. **Ne alacritty'de monoton bir satır sayacı var, ne de kayan satırı
   bildiren bir olay.** `grid/mod.rs`'in `pub` yüzünde `history_size`,
   `total_lines`, `display_offset` var; `scroll_up` hiçbir şey yayınlamıyor
   ve `EventListener`'ın olay listesinde kaydırma yok.
3. **Türetilen indeks üç yerde kırılıyor.** `abs = history_size + row`:
   - **Doygunluk** — `increase_scroll_limit` `max_scroll_limit`'e dayanınca
     `history_size` büyümüyor (`grid/mod.rs:176`), sonraki her kayan satır
     saklanan bütün çıpaları bir satır yanlışlıyor. Varsayılan `scrollback`
     **10 000** (`docs/AYARLAR.md`), yani bu bir köşe durumu değil günlük
     kullanımın olağan hâli.
   - **Reflow** — `grid/resize.rs`'in `grow_columns`/`shrink_columns`'ı
     satırları birleştirip bölüyor; pencereyi bir kez yatay boyutlandırmak
     bütün çıpaları kaydırır.
   - **`clear_history`** — geçmişi düşürüyor, dış listenin haberi olmuyor.
4. **Hücrenin yan tablosu kaydırmadan ve reflow'dan sağ çıkıyor.** OSC 8
   hyperlink'i `CellExtra`'da yaşıyor (`term/cell.rs:125`, hücrenin 24 baytı
   oynamıyor) ve reflow hücreleri **değer olarak** taşıdığı için satırla
   birlikte gidiyor; satır sıfırlanınca (`reset(&template)`) düşüyor.
   Yani ızgaranın kendisi dayanıklı bir çıpa taşıyabiliyor.

Bu ikisi **ayrı** sorunlardır ve 009'un açık bıraktığı "kendi okuma
döngümüz" kapısı yalnız birincisini kapatıyor: baytları işaretin tam üstünde
bölmek çıpayı doğduğu anda kesinleştirir, ama saklandıktan sonraki
kaymasına hiçbir şey yapmaz. Karar 1 bu yüzden yeniden kurulmalı.

## Mevcut Mimari

```
zsh (bateri.zsh precmd/preexec)
   │ OSC 133 A/B/C/D;kod
   ▼
okuyucu thread ── TappedPty::read ── Scanner::feed ──► ShellState (yaprak kilit)
   │  (baytlar dokunulmadan geçer)                          │
   ▼                                                        │ Session::shell_state()
EventLoop::advance ──► Term (grid, scrollback)              │   → TÜKETİCİ YOK
   │                                                        ▼
   │ Session::frame(sink) → Cell{col,row,…} + Cursor
   ▼
bt-gpu  Frame ──► cell_bg / cell pipeline'ları
```

Eksik kenar tek: `ShellState`'ten (ya da onun yerine gelecek blok
kaydından) `frame()` sınırına ve oradan `bt-gpu`'ya giden yol.

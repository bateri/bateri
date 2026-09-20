# Ekranın geri dönüşü — Bağlam

## Mevcut Durum

Tab tamamlama listesi ızgaraya basılıyor ve ekranı kaydırıyor; liste
kalktığında geriye bir delik kalıyor. `docs/YOL-HARITASI.md:378`'de "sete
bağlanmamış borç" olarak duruyor, kökü 011'in kayıtlı bedeline
(`.tasks/011-tabana-yapisik-icerik/discussion.md:341-345`) ve 012'nin Karar
3a'sına (`.tasks/012-input-dock/plan.md:107`, "tamamlama listesi ızgarada
kalır") bağlı.

Bugünkü yol üç parçadan oluşuyor:

1. **Ayna satırı bastırıyor** — kabuk `Input` safhasındayken ve ayna canlıyken
   çıpa satırından imlecin satırına kadar hücreler `frame()` sink'ine
   uğramıyor (`crates/bt-core/src/shell.rs:1007` `suppressed_input`, aralık
   `crates/bt-core/src/session.rs:1549-1587`). Tamamlama listesi imleç
   satırının **altına** düştüğü için aralığın dışında kalıyor ve ızgarada
   görünüyor — bu bilinçli (`session.rs:1538-1543`: `saturating_sub(1)`
   olmasaydı "o satır tamamlama listesinin ilki olurdu").
2. **İçerik tabana yaslanıyor** — `Cursor::content_rows` kaç satırın dolu
   olduğunu sınırdan veriyor, `origin = rows - content_rows` ötelemeye
   çevriliyor ve tek `setViewport` bütün pipeline'ları kaydırıyor. Üstte kalan
   `origin` satır **boş** çiziliyor.
3. **Kayma tek yönlü** — öteleme hedefi düşünce (içerik büyüyünce) süzülüyor,
   yükselince (içerik daralınca) snap'liyor. Kural 011'in gözle kontrolünden
   çıktı (`4c291ee`: "vim'e GİRERKEN süzülmesi isteniyor, tuhaf olan çıkışta
   kabuğun aşağı inmesi").

Tamamlama listesi ızgarayı kaydırınca üstteki çıktı scrollback'e düşüyor;
liste silinince `content_rows` daralıyor, öteleme snap'liyor ve içerik dock'un
üstüne yapışıp **üstünde boş bir şerit bırakıyor**. Kullanıcı bunu "çıktım
gitti" diye okudu (2026-09-19, dört ekran görüntüsü).

Bir kez denendi ve geri alındı: `2fdca50` tabana yaslamayı "geçmiş boşken"e
sınırladı, yani boşluğu içeriğin üstünden **altına** taşıdı; `27a0b98` onu
geri aldı — "delik yer değiştirdi, kapanmadı", çünkü orası (içerik ile dock
arasındaki şerit) daha bozuk görünüyor.

## Motivasyon

Kullanıcının koyduğu kural (2026-09-20): **Tab tamamlama listesi kapandığında
ekran Tab'dan önceki hâline dönsün**, ve dönüş **kayarak** olsun — bugün yeni
içerik gelince aşağıdan yukarı akan hareketin tam tersi yönde.

Yol haritası bu maddeyi "gerçek çare listeyi ızgaraya hiç düşürmemek (aynanın
altıncı kanalı ya da overlay)" diye kapatmış ve gerekçesine "kaybı hiçbir
terminal geri getiremez" yazmıştı. Aşağıdaki ölçüm o ikinci cümleyi çürütüyor:
**kayıp yok** — listenin ittiği satırların tamamı scrollback'te duruyor ve
`2fdca50`'nin kendi commit iletisi de bunu söylüyordu ("tamamı bir tık
yukarıda scrollback'te duruyordu ve tekerlek geri getiriyordu"). Geri
getirilemeyen şey zsh'in satırları **yeniden basması**; göstereceğimiz veri
elimizde.

Bu, overlay'siz ve tel değişikliksiz bir çare açıyor: üstte kalan boşluğu boş
bırakmak yerine **geçmişin en yeni satırlarıyla** boyamak. Kullanıcının
istediği "ters yönde kayma" da yeni bir animatör istemiyor — bugün snap'lenen
yönün, doldurma varken süzülmesi demek.

## Kanıt

Ölçümler 2026-09-20, gerçek `zsh -i`, `pty.fork`, `ROWS=12 COLS=80`, boş
`ZDOTDIR` (`PS1='%% '`, `RPS1=''`, `setopt NO_BEEP`), `compinit`
doğrulandı (`COMPINIT=1`), dizinde 30 dosya (`alfa_00`…`alfa_29`), komut
`ls alfa_` + Tab.

**1. Tab basımı — 312 bayt, ekran dolu ve ekran boş koşularında birebir aynı:**

```
\r\r\n \e[J
alfa_00  alfa_04  …\r\n   (dört satır, aralarında \r\n)
\e[4A \e[0m\e[27m\e[24m \r \e[2C ls alfa_ \e[K
```

- `LF=4`, **`CSI S` (scroll up) yok, `ESC D` yok, alternatif ekran yok.**
- Yani **zsh kaydırma komutu göndermiyor**; kaydırma terminalin kararı —
  imleç son satırdaysa `\n` ekranı kaydırır, değilse kaydırmaz. Bu konuda
  iTerm'den farkımız yok.
- `\e[4A` ile imleç komut satırına geri çıkıyor (`ALWAYS_LAST_PROMPT`), yani
  liste **imlecin altında** duruyor ve bastırma aralığının dışında kalıyor.

**2. Liste nasıl kapanıyor — üç kol ölçüldü:**

| Tuş | Ne oluyor | Bayt |
|---|---|---|
| harf (`0`) | **liste silinmiyor**, ekranda kalıyor; yalnız harfin echo'su | 1 |
| `ESC` | **hiçbir şey gönderilmiyor**, liste duruyor | 0 |
| `Ctrl-C` | `\e[?2004l \r\r\n \e[J` + yeni prompt (`precmd` koşuyor) | 149 |
| `Enter` | `\e[?2004l \r\r\n \e[J` + komutun çıktısı + yeni prompt | 160 |

İki kapanış kolunda da `\e[J` var, **sıfır dosya adı** geri basılıyor ve
`\r\r\n` yüzünden yeni prompt eskisinin bir satır altına düşüyor (bu +1 phase
içinde ölçülecek). Yani delik iki yerde doğuyor: iptalde ve çıktısı kısa bir
komutun Enter'ında.

**3. Kasten temizleme aynı belirtiyi üretiyor — ayrımın zorunlu olduğu yer:**

- zsh `clear-screen` (Ctrl-L): `\e[H\e[2J…` — **`3J` yok**, yani scrollback
  korunuyor.
- `clear(1)`: `\e[3J\e[H\e[2J` — `3J` ile scrollback siliniyor, ama
  arkasından gelen `2J` ekranı yine geçmişe kaydırıyor.

**4. alacritty `2J`'yi geçmişe kaydırıyor** (`alacritty_terminal-0.26.0`):
`Term::clear_screen(ClearMode::All)` → `Grid::clear_viewport()`
(`src/term/mod.rs:1788-1802`) → `scroll_up(region, positions)`
(`src/grid/mod.rs:309-333`). Yani **Ctrl-L de `history_size()`'ı büyütüyor.**

Bu dördüncü ölçüm, "geçmişte satır var ve ekranda boşluk var → doldur"
kuralını tek başına geçersiz kılıyor: Ctrl-L'den sonra ekran silinmiş olmasına
rağmen geçmişin en yeni satırları tam da silinen satırlar oluyor ve doldurma
Ctrl-L'yi hiç çalışmamış gösterirdi. Doldurmanın **kasten temizlemeyi ayırt
eden bir sinyale** ihtiyacı var.

## Mevcut Mimari

```
PTY okuyucu thread
  └─ Scanner::feed (bt-core/src/shell.rs:1468)   ESC ] … kolları: 133, 8133, 7
       └─ baytlar aynen alacritty Term'e            CSI hiç tanınmıyor
                                                    (ScanState::Escape, ']' dışı → Ground)
frame() (bt-core/src/session.rs)
  ├─ Term kilidi: hücreler + Cursor::content_rows + çıpa taraması
  └─ sink → bt-gpu Frame

bt-gpu
  ├─ link.rs  origin_target = rows - content_rows  → Slide animatörü
  │            (tek yön: hedef düşünce süzülür, yükselince snap)
  ├─ frame.rs set_origin_rows (kesirli, aygıt ızgarasına yuvarlanıyor)
  └─ renderer.rs encode_pass → tek setViewport (ızgara) + ikinci setViewport (dock)
```

Boşluğun bugünkü tanımı: `origin` satır kadar **hiç çizilmeyen** alan.
Bu setin dokunduğu yer tam olarak orası — boşluğun **ne olduğu** değil, **ne
boyandığı**.

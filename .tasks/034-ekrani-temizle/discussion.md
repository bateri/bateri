# Ekranı temizle (⌘K) ve eksik standart davranışlar — Tartışma

## Karar 1: Temizlemeyi kim yapar — terminal mi, kabuk mu?

### Seçenek A: Terminal tarafı, blok korunur (Terminal.app'in yolu)

⌘K kabuğa hiçbir şey göndermiyor. `bt-core`'da tek `Term` kilidi turunda:
pencere dibe döner (kesir/nesil dahil, `reset_scroll`), korunacak ilk satırın
üstündeki satırlar ızgaranın tepesinden dışarı kaydırılır
(`grid_mut().scroll_up`, imleç aynı satır kadar yukarı), geçmiş silinir
(`ClearMode::Saved`) ve mevcut `2J` nesli bir artırılır — yani bayrağın dört
tüketicisi (doldurma kapısı, `scrolled = 0`, damga, `clear_boundary`) gerçek
bir `2J`'deki gibi davranır. Korunan satırlar: **kabuk girdi safhasındaysa
prompt çıpasının satırından**, değilse (komut koşuyor, entegrasyonsuz kabuk,
çıpa bulunamadı) **imlecin satırından** aşağısı.

**Artıları:**
- Kabuktan bağımsız: zsh (dock'lu ya da `blocks`), `/bin/sh`, komut koşarken
  (`tail -f`) aynı yol; hiçbir bayt PTY'ye gitmiyor, yani "hiçbir komut
  çalışmaz" yapısal.
- Zamanlama yarışı yok: `2J`'nin geçmişe ittiği ekranı sonradan silme sorunu
  (alacritty'nin kendi Cmd+K'sının kusuru) doğmuyor, çünkü `2J` hiç yok.
- Prompt satırının OSC 8 çıpası **yeniden basılmıyor, korunuyor**: blok
  şeridi, `caret_home` devri, bastırma ve `SuppressedInput::from_anchor`'un
  tabanı el değmeden çalışıyor; çok satırlı giriş (`PREBUFFER`, sarma)
  çıpadan başladığı için bütünüyle kalıyor.
- Betiğe (`assets/shell/`) ve `frame()`'e dokunmuyor.

**Eksileri:**
- ZLE ekranın değiştiğini bilmiyor; göreli imleç hareketleri korunan blok
  içinde kaldığı sürece doğru (blok çıpadan başladığı için kalıyor).
  Komut koşarken göreli hareketle kendi arayüzünü yeniden çizen bir program
  (birincil ekranda Claude Code/ink) bir kare bozuk çizebilir — her
  terminalde aynı; ^L onu da onarır.
- `Grid`'in düşük seviye çağrılarını kullanıyor (`scroll_up`, imleç), yani
  alacritty'nin kaydırma bölgesini (DECSTBM) atlıyor — bilinçli: temizleme
  bölgeye değil ekrana ait.

### Seçenek B: Kabuk tarafı — `zle clear-screen` + bekleyen geçmiş silme

⌘K dock komut kanalına yeni bir alt komut gönderir (`CSI 8133 ~ c BEL`,
widget `zle clear-screen`), kanal kapalıysa `\x0c`. Kabuğun `2J`'si ekranı
geçmişe iter; `bt-core` "geçmişi de sil" isteğini bekletir ve `2J`'nin
uygulandığı **damga karesinde** (`observe_screen_clear`'ın `UNSTAMPED`
kolu) `ClearMode::Saved` ile tüketir.

**Artıları:**
- Prompt kabuğun kendisi tarafından yeniden basılıyor; Ctrl-L yolu (ve
  `clear_boundary` istisnası) zaten sınanmış.

**Eksileri:**
- Betik değişiyor (pahalı sınıf; bash/fish betikleri henüz yok, yani
  entegrasyonsuz kabukta yedek `\x0c`'ye kalıyor ve kullanıcı ^L'i yeniden
  bağlamışsa başka bir şey yapıyor).
- `vicmd`'de kanal bağlı değil (widget bağlaması yalnız
  `main`/`emacs`/`viins`), `\x0c` yedeği orada da gerekiyor.
- İsteğin ömrü tasarlanmalı: kabuk hiç `2J` basmazsa bekleyen istek sonraki
  bir Ctrl-L'de geçmişi sessizce siler; `send_input`'ta düşürmek ya da süre.
- Komut koşarken çalışmıyor: `\x0c` koşan programın girdisine yazılırdı
  (`cat`), yani o kol ayrıca terminal tarafı bir temizlik istiyor —
  Seçenek A'yı zaten içeriyor.
- `frame()`'e (kare yolu) yeni bir kol ekliyor.

## Karar 2: Alternatif ekranda (vim, htop, less) ⌘K ve ⌥⌘K

Ekranın sahibi uygulama. Birincil ızgaranın geçmişi `Term::inactive_grid`'de
ve alan özel (`context.md`), yani vim açıkken **birincil** geçmiş kitaplığı
çatallamadan silinemiyor; alternatif ızgarayı temizlemek ise vim'i ^L'e kadar
çöp göstermeye bırakır. Seçenekler: (a) öğeler devre dışı (gri), (b) istek
alternatif ekrandan çıkışa ertelenir, (c) alternatif ızgarayı temizle.

## Karar 3: Envanterden bu sete ne girer

`context.md` → Envanter. Ölçüt `CLAUDE.md` → "Boşlukta kullanıcı tarafı
seçilir": tek menü öğesi olan ve ⌘K'nin mekanizmasını ya da depoda hazır bir
parçayı kullanan her şey bu sete; kendi tasarımını isteyen yol haritasına.

## Karar 4: Menüde yer ve kısayollar

Terminal.app'in yerleşimi: Edit'te Clear to Start (⌘K) ve Clear Scrollback
(⌥⌘K) ile Paste Escaped Text (⌃⌘V); View'da Scroll to Top (⌘Home), Scroll
to Bottom (⌘End), Page Up (⌘PgUp), Page Down (⌘PgDn). Seçiciler kendi
adlarımız, karşılayan `TerminalWindow` (Find ▸ emsali, 033 Karar 10);
`keyDown:`'ın Cmd izin listesi değişmiyor (menü tuşu önce yakalıyor, ⌘A
emsali). Home/End'in yutulması ve `bt_core::Arrow`'un "genişletilmez"
değişmezi el değmiyor: ⌘Home/⌘End bir **menü** kısayolu, tuş kodlaması değil.

## Muhakeme (2026-09-25)

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ — A, B'den açıkça basit; tek fonksiyon ⌘K ile ⌥⌘K'yi karşılar |
| Codebase-fit | SORUNLU — A doğru yön, üç tamamlanma boşluğu (çıpa yürüyüşü, `Term` yan etkileri, arama) |
| İşletme | SORUNLU — A doğru yön, üç sessiz bozulma (seçim/imleç, arama, tek artırıma yaslanan doğruluk) |

**Kabul edilen itirazlar → plan değişikliği:**
- `anchor_row_at_or_above` imlece **en yakın** çıpalı satırı buluyor; bağlantı
  `preexec`'e kadar açık olduğu için sarılan/çok satırlı girişte bu imlecin
  kendi satırı (session.rs:1793, `block_row_continues` :2196) → korunacak ilk
  satır, aynı blok kimliğini taşıyan bitişik satırlarda **yukarı yürüyerek**
  bulunur; bloğun başı ekranın üstündeyse (geçmişte) hiçbir satır dışarı
  kaydırılmaz, yalnız geçmiş silinir.
- `Grid::scroll_up` seçimi, imleci ve hasarı taşımıyor (alacritty
  `term/mod.rs:770` `scroll_up_relative` taşıyor; `grid/mod.rs:252`
  taşımıyor) ve `ClearMode::Saved` yalnız geçmişe değen seçimi süzüyor →
  seçim (ızgara **ve** dock, "pencerede tek seçim") koşulsuz temizlenir, imleç
  açıkça düşürülür, `saved_cursor` (DECSC) aynı miktarda düşürülüp 0'a
  kırpılır, kare kilit bırakıldıktan sonra `request_frame` ile istenir.
- "Dibe dön = `reset_scroll`" yanlış adlandırmaydı (`reset_scroll` ofsete
  dokunmuyor; `Grid::scroll_up` ofseti büyütüyor, grid/mod.rs:264) →
  `send_input`'un ikilisi açıkça: `Scroll::Bottom` + `reset_scroll`,
  kaydırmadan **önce**.
- Arama kendiliğinden yeniden başlamıyor (`Adapter::search_changed` yalnız
  `Wakeup`/`set_terminal_options`/`resize`'dan) ve geçmiş 0 → 0 kolunda
  `ledger_shift` `Still` döndürüp kaymış satırı geçerli gösterirdi
  (search.rs:546–560) → temizleme defter neslini artırır, geçerli eşleşmeyi
  açıkça kaybettirir ve `search_changed` çağırır; bekçisi o kol.
- `screen_clears`'ın doc'u "artıran yalnız burası" diyor (session.rs:1312) →
  ikinci yazar adlı tek bir yöntemden geçer, artırım `Term` kilidinin
  **altında** ve uygulamadan **sonra**, doc iki yazara göre yeniden yazılır.
- ⌥⌘K ayrı yol değil: aynı fonksiyon, dışarı kaydırılacak satır sıfır.
- ⌘Home/⌘End `bt-core`'a yeni API istemiyor: `scroll_page(±i32::MAX)`
  (`saturating_mul`, session.rs:4969; `scroll_locked` kırpıyor ve bant
  kuralıyla dibe iniyor) — süzülme nesli de o yoldan artıyor.
- Paste Escaped Text'te `shell_quote`'un "patolojik" diye kabul ettiği sınır
  (`\` + satır sonu satır devamıdır, quote.rs) pano metninde olağan → kural
  aşağıda (Karar 3), metin olduğu gibi korunur.
- Gözle kontrol "prompt en üstte" dememeli: bateri'de içerik tabana yaslı,
  prompt ve dock dipte kalır, üstü boşalır (bugünkü Ctrl-L'in görüntüsü).

**Reddedilenler:**
- "`2J` neslini artırma, yalnız `clear_boundary`'yi sıfırla + `Wakeup`
  gönder" (Sadelik) — gözlem doğru (geçmiş boşken doldurma ve kayma sayısı
  kendiliğinden sıfır), ama nesil temizlemeyi bayrağın **bütün** bugünkü ve
  gelecekteki tüketicilerine tek kelimeyle söylüyor; üç tüketiciyi "geçmiş
  boş olduğu için tesadüfen doğru" bırakmak, 017'nin damga kırpmasının
  kapattığı türden sessiz bir bağımlılık. Bedeli tek adlı yöntem ve bir doc.
  Sınama dört tüketici için değil tek değişmez için yazılır (`plan.md` R1.4).
- `Event::Wakeup`'ı kilit altında tetiklemek (Sadelik) — `Wakeup` "PTY
  çıktısı geldi" demek ve hasar diker; defter nesli + `search_changed` +
  `request_frame` üçlüsü aynı işi adıyla yapıyor.

## Karar (2026-09-25, otonom akış)

- **Karar 1 → ✅ Seçenek A**, Muhakeme'nin kabul edilen itirazlarıyla — tek
  `Term` kilidi turunda terminal tarafı temizlik, kabuğa bayt gitmez. Kabuktan
  ve safhadan bağımsız (dock'lu zsh, `blocks`, `/bin/sh`, koşan komut),
  zamanlama yarışı yok, betiğe ve `frame()`'e dokunmuyor.
  **Reddedilen:** B — betik (pahalı sınıf; bash/fish betikleri yok, yedek
  `\x0c` kullanıcının ^L bağlamasına ve `vicmd`'e kalıyor), bekleyen isteğin
  ömrü, `frame()`'e yeni kol; komut koşarken zaten A'yı içermek zorunda.
- **Karar 2 → ✅ (a) devre dışı.** Alternatif ekranda Clear to Start, Clear
  Scrollback ve dört kaydırma öğesi gri (`validateMenuItem:`,
  `Session::alt_screen`); `bt-core` fonksiyonu da alternatif ekranda hiçbir
  şey yapmaz (çağıran değişmezi, `scroll_locked`'ın `None` emsali). Kısıt
  olgu: birincil geçmiş `Term::inactive_grid`'de ve alan özel. Ürün tarafı:
  gri öğe dürüst bir "burada olmaz"; alternatif ızgarayı temizlemek vim'i ^L'e
  kadar çöp gösterirdi. **Reddedilen:** (b) ertelemek — B'nin reddedilen
  "isteğin ömrü" sorusunu geri getiriyor; (c) — yukarıda.
- **Karar 3 → ✅** Bu sete: ⌘K Clear to Start, ⌥⌘K Clear Scrollback,
  ⌘Home/⌘End Scroll to Top/Bottom, ⌘PgUp/⌘PgDn Page Up/Down, ⌃⌘V Paste
  Escaped Text. **Paste Escaped Text'in kuralı:** satır sonu taşımayan metin
  Finder damlasının kaçırmasından (`quote::shell_quote`, görüntü aynı);
  satır sonu taşıyan metin bütünüyle **tek tırnakla** sarılır (`'` →
  `'\''`) — POSIX'te tek tırnak satır sonunu harfi harfine taşır, yani metin
  birleşmez ve bracketed sarma çalıştırmaz. Yol haritasına: komut işaretleri
  üstünde gezinme (⌘↑/⌘↓, Select Between Marks, ⌘L, son çıktıyı kopyala) ve
  tıklanabilir bağlantılar (set satırları); zil ve Reset (borç kalemleri) —
  `docs/YOL-HARITASI.md`. Gerekçe `context.md` → Envanter.
- **Karar 4 → ✅** Edit'te Clear to Start/Clear Scrollback (Select All'ın
  altında) ve Paste Escaped Text (Paste'in altında); View'da dört kaydırma
  öğesi. Temizleme ve kaydırma seçicileri `TerminalWindow`'da (Find emsali);
  Paste Escaped Text `BateriView`'da (`paste:` emsali — arama alanı odaktayken
  alanın düzenleyicisi yapıştırmayı karşılamalı, terminal değil). Cmd izin
  listesi, Home/End'in yutulması ve `Arrow`'un değişmezi el değmez.

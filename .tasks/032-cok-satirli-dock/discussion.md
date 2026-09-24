# Çok satırlı dock — Tartışma

Yön kullanıcıyla kararlaştırıldı (ürün kararı, 2026-09-24): dock çok satırı
kendisi gösterir ve yukarı doğru büyür. Aşağıdaki kararlar o yönün **nasıl**
yürüyeceği; ürün tarafında kalan her boşluk kullanıcı lehine okundu
(`CLAUDE.md` → İş akışı).

## Karar 1: Dock nerede büyüyor?

### Seçenek A: Çizim tarafında — PTY sabit, ızgara ötelenir

PTY'nin boyu bugünkü gibi `DOCK_ROWS`'la ayrılır ve **hiç değişmez**. Dock ek
giriş satırlarını yalnız çizimde alır: bandın boyu `dock_px(2 + ek)`, ızgara
aynı `ek` satır kadar yukarı ötelenir (`bt_gpu::Origin`'in tek sahibi olduğu
öteleme; doldurma bandı ve 027'nin kesriyle aynı toplam). Izgara doluysa
tepedeki `ek` satır pencerenin dışına kırpılır — geçici, giriş bitince döner;
değilse içerik yalnız yukarı kayar ve üstteki boşluğun doldurma bandı o kadar
kısalır. Geçiş kayma animatörüyle.

**Artıları:**
- Kabuk hiçbir şey görmez: SIGWINCH yok, zsh prompt'u yeniden çizmez, sarmalı
  geçmiş yeniden akmaz — eski gerekçenin ("nefes alan ekran") üçü de konusuz.
- Gerekli parçaların hepsi var: negatif orijin sınanmış
  (`a_negative_viewport_origin_draws_and_clips_from_the_top`), caret'in yuva
  seçimi örtüşmeye bakıyor (`push_caret`, band boyuna kendiliğinden uyar),
  fare eşlemesi `Origin`'den okuyor.
- Kullanıcının üstteki çıktıyı görme ihtiyacı korunuyor: kırpılan en **eski**
  satırlar, komutun hemen üstündeki çıktı yerinde.

**Eksileri:**
- İki "dock satırı" kavramı doğar (PTY'nin ayırdığı / çizilen); karışmasın
  diye ad ayrılmalı.
- ~~`origin_target` ve `Motion::sync`'in `u16`'sı işaretliye geçer.~~
  (panelden sonra kalktı: birleştirme `set_origin`'de, `## Muhakeme`)
- Kırpılan ızgara satırlarına bu sürede tıklanamaz (ekranda yoklar).

### Seçenek B: PTY resize — bant ızgaranın satırlarından düşülür

Her ek satır bir `TIOCSWINSZ`.

**Eksileri:**
- Yazarken nefes alan ekran; zsh SIGWINCH'te satırı yeniden çizer (tam da
  düzenlemenin ortasında), sarmalı geçmiş yeniden akar. `docs/YOL-HARITASI.md`
  bu gerekçeyle ertelemişti. Reddedilmeye aday.

### Seçenek C: Opak bandı ızgaranın üstüne büyütmek (örtme)

**Eksileri:**
- Kullanıcının komutu yazarken görmek istediği şeyi — komutun hemen
  üstündeki çıktıyı — örter (aynı borç kalemi). Reddedilmeye aday.

## Karar 2: `PREBUFFER` — `for`, heredoc, `\`-devam

Ölçüm (`context.md` → Kanıt): bu üç hâlde `BUFFER` tek satır, önceki satırlar
`PREBUFFER`'da ve ZLE onları artık düzenlemiyor. Yalnız `BUFFER`'daki `\n`'i
ele almak kullanıcının dört örneğinden birini karşılar.

**Öneri:** ayna yedinci bir gövde taşır (`b64(PREBUFFER)`, yük bütçesine
girer); dock onu düzenlenebilir satırların **üstünde**, aynı renkte ve aynı
girintide çizer. Seçilebilir ve kopyalanabilir (bütün döngüyü kopyalamak
beklenen şey), ama ZLE onu düzenleyemediği için **salt okunur**: tıklama
caret'i oraya taşımaz, `PREBUFFER`'a değen bir seçimde ⌫/⌦/yazma/⌘X seçimi
kaldırıp bugünkü yoldan gider (031 Karar 8'in "başka her tuş" kolu).
Dock'un devam satırları işaretsiz, metin sütunundan başlar. Kullanıcının
`PS2`'sine **dokunulmaz** (panelden sonra; `## Muhakeme`): ızgaradaki
`for>`/`heredoc>` bağlamı geçmişte kalır.

## Karar 3: Uzun mantıksal satır — sarma mı, yatay pencere mi?

**Öneri: sarma**, tek satırlık uzun komut dahil. Dock bir editör yüzeyine
dönüşüyor ve çok satırlı bir düzenleyicide satır başına yatay kaydırma hem
tuhaf hem ızgaranın (zsh de sarıyor) davranışının tersi. Tutarlılık için tek
kural: dock komutun tamamını gösterir; `window_skip` emekli olur. Görünür
sonucu: bugün yana kayan uzun tek satırlık komut artık dock'u iki satıra
büyütür — kullanıcının lehine (komutun tamamı görünür). Devam satırları
`TEXT_COL`'dan başlar (asma girinti, metin sütunu hizalı); geniş karakter
yarılanmaz, sığmıyorsa bir alt satıra geçer.

## Karar 4: Tavan

**Öneri:** giriş satırları pencerenin ızgara satırlarının **yarısını** aşmaz
(`DOCK_MAX_SHARE`, bir tasarım sabiti — emsali `CONTEXT_SCALE`/`GUTTER_PT`;
oran, mutlak sayı değil, yani pencereyle ve puntoyla ölçeklenir; ölçülmüş bir
sayı değil). Gerekçe: komutun yazıldığı yüzey ile onun bağlamı olan çıktı eşit
kalsın, editör pencereyi yutmasın. Aşan girişte dock kendi içinde **dikey
pencere** açar ve caret'i izler (bugünkü yatay `window_skip`'in dikey ikizi,
durumsuz: caret'in satırı görünür kalacak en küçük kayma). Bastırma yine
ızgaradaki bütün satırları kapsar. En az bir giriş satırı her zaman.

## Karar 5: Büyüme ve küçülmenin animasyonu

**Öneri:** bandın ek satır sayısı `bt-gpu::motion`'da **kendi** `Slide`'ında
süzülür (imleç, öteleme ve `glide`'ın yanında dördüncü `Slide`, aynı stil, aynı `settled()`
kapısı); ızgaranın çizilen orijini = içerik ötelemesi − bandın o anki ek
yüksekliği. Izgaranın alt kenarı bandın üst kenarıyla **yapısal olarak**
çakışır — iki ayrı animatör olsaydı arada boşluk ya da örtüşme doğardı.
İki yön de süzülür: bandın boyu bir panel boyu, içerik değil, yani 011'in
"daralan içerik snap'ler" gerekçesi (aşağı iniş düşme gibi okunuyor) buraya
uymuyor — satır silince bandın kaybolup ızgaranın zıplaması tam da o kuralın
önlemek istediği sıçrama olurdu. Enter'da iki hareket birbirini neredeyse
götürür (bant küçülür, bastırılan satırlar ızgaraya döner). Hareketi Azalt,
`cursor_motion = "snap"` ve geometri değişimi snap'ler; 240 ms tabanı
(`docs/ARASTIRMA.md`) ve gözle kontrol.

## Karar 6: Seçim ve tıklama çok satırda

**Öneri:** isabet testi (satır, sütun) → karakter indeksi, tek düzen
yürüyüşünden. Sürükleme satırlar arası seçer; vurgu 031'in satır koşuları
(her görsel satıra bir koşu, yuvarlak köşeli tek parça şekil zaten çok
satırı biliyor). Çift tıklama kelime (değişmez); **üçlü tıklama mantıksal
satırı** (`\n`'ler arası, sarılmış görsel satırlarıyla) seçer — ızgaranın
üçlü tıklaması da sarılmış mantıksal satırı seçiyor ve macOS metin
alanlarının paragraf seçimi aynı; bütün `BUFFER` ⌘A'nın işi olarak kalır.
Tıkla-caret ve `d;S;E;L` teli zaten düz karakter indeksi, değişmez. ↑/↓
ZLE'nin (`BUFFER` içinde satır gezer), terminal araya girmez.

## Karar 7: Bastırmanın satır aritmetiği

**Öneri:** `bt-core`'da **tek** düzen fonksiyonu (`dock::layout`): görüntünün
metnini satır sonlarında böler ve verilen genişlikte sarar; iki
parametrizasyonu var — dock (ilk ve devam satırları `TEXT_COL`'dan) ve ızgara
(zsh'in düzeni: ilk satırın başı imlecin ızgaradaki sütunundan gözlenir,
`BUFFER`'ın devam satırları 0. sütundan; ölçüldü, `context.md` → Kanıt;
`PREBUFFER` ızgara hesabına girmez — panelden sonra daraldı). Dock çizimi, isabet testi, 030'un
konum eşlemesi ve bastırmanın `to`/`floor`'u aynı fonksiyondan okur; sütun
bölmesi (`suppress_to`/`suppress_floor`'un bugünkü formülü) kalkar.
`PREBUFFER` doluysa üst taban çıpanın satırıdır (bağlantı `preexec`'e kadar
açık, yani bütün komut çıpayı taşıyor). Dock'un çizeceği giriş satırı sayısı
`frame()`'de, bastırma kararıyla **aynı okumada** hesaplanır, `Cursor`'la
sınırdan çıkar ve `Session::dock`'a argüman olarak gider (`caret_in_dock`'un
emsali: cevap hesaplandığı yerden geçer) — iki kilit turundan türetilseydi
bant ile dock'un satırları bir kare ayrışabilirdi. `blank_mirror`'ın
`cols_before == 0 && cols_after == 0` tanımı satır farkında değil; yeniden
tanımlanır ("görüntünün hiç karakteri yok"), 025'in yapıştırma senaryosu
(sondaki satır sonunda duran caret) bekçisiyle.

## Karar 8: Çok satırlı yapıştırmanın bayat aynası

`can_be_typed` satır sonlu yükü bracketed yolda tutmak zorunda (çıplak satır
sonu komutu koşturur). `bracketed-paste-magic` aynayı bir tuş boyunca bayat
bırakıyor ve kabuk tarafının üç çaresi ölçülüp kapandı — hepsi ZLE'nin
**içinden** koşuyordu. Kullanıcının bildirdiği "satır ızgaraya fırlıyor"
belirtisi bu.

**Öneri:** terminal, dock satırın sahibiyken ve düzenleme yeteneği bu
prompt'ta görülmüşken (031'in `w`'si), satır sonlu bir bracketed
yapıştırmanın **arkasından** sarmalayıcının widget'ına tek bir tazeleme
komutu gönderir (`CSI 8133 ~ r BEL`; kapısı panelden sonra 031'in tam
düzenleme kapısı, `## Muhakeme`). Bugünkü widget (`__bateri_dock_edit`)
tanımadığı yükte `BUFFER`'a dokunmadan döner ve **her koşulda** sonunda
aynayı basar, yani yol bugünkü betikle de çalışır; betiğe düşen yalnız `r`'yi
tel başlığında adıyla yazmak. Dizi yapıştırmadan sonra okunuyor; önce
`bracketed-paste-magic`'in kendi okuması yakalayıp `zle -U` ile geri
itebilir, yani aynanın yapıştırmanın **sonucunu** taşıdığı bir varsayım değil
ölçüm sorusu. Ölçülerek doğrulanır
(oh-my-zsh'li gerçek pencere); tutmazsa bugünkü güvenli yol (tazelik kapısı
satırı bir tuş boyunca ızgarada tutar) kalır ve kalem adıyla yazılır.

## Karar 9: Değişmeyenler

- `DockStatus::Control` kalır: dock `^A`'yı hâlâ çizmiyor, satır ızgarada
  okunur `^A`'yla. `\n` artık kontrol sayılmaz, sekme bugünkü gibi.
- `Unavailable` (yük sınırı) kalır; `PREBUFFER` bütçeye girer.
- Alternatif ekranda dock yok, çok satır da yok.
- Bağlam satırı bandın **altında** kalır; saç çizgisi bandın tepesinde ve
  giriş bloğu ile bağlam satırı arasında (giriş satırlarının kendi arasında
  çizgi yok — tek bir editör).

## Karar 10: 030'un yazım efektleri

**Öneri:** efektlerin anahtarı sütun değil **konum** (satır, sütun); `diff`
düz metin üstünde kalır (tek bitişik ekleme/silme) ve düzen eşlemesi indeksi
konuma çevirir. Eklemeden sonraki metin sarma yüzünden satır değiştirirse
kayma iki eksende; eşlenemeyen hâl (satır sayısı atladı, geçmişten komut)
bugünkü gibi anında (`Reset`).

## Muhakeme (2026-09-24)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU (hafif) — çekirdek doğru; işaretli öteleme gereksiz, `PS2` dayatması mekanizmaya hizmet etmiyor, ızgara parametrizasyonu geniş |
| Codebase-fit | SORUNLU — dock hücreleri tepeye çıpalı ve push anında pişiyor, süzülen bantla çelişir; dock'un fare yolu `Origin`'den okumuyor; `e`→`u` boşluğu ve yön kuralı titreme doğurur |
| İşletme | SORUNLU — `r` komutunun kapısı eksik (vicmd'de tampon bozulur); `Multiline`'ın arkasında dört sessiz kırılma; kapılar bu yolu görmüyor |

**Kabul edilen itirazlar → plan değişikliği:**
- Öteleme işaretliye geçmesin (Sadelik 1, Codebase notu) → `Motion::sync`
  ve `origin_target` `u16` kalır, yön kuralı yalnız içeriğe bakar; birleştirme
  iki kare yolunun ortak noktasında: çizilen orijin = `motion.origin()` −
  bandın o anki ek satırı (`LinkDelegate::set_origin`), ızgara caret'inin
  hedefi `cursor.row + origin_target − ek hedef`.
- Dock'un yerleşimi süzülen bantla çelişiyor (Codebase 1) → dock hücreleri
  bandın **dibinden** sayılır (bağlam satırı dipten ilk), bandın o anki
  yüksekliği `Frame`'e **iki** kare yolunun da yazdığı bir değer ve encode
  anında okunur (`fill_origin_px`'in emsali); `set_dock_top` ve
  `dock_caret_at` aynı değerden; `dock_height`'in boşluğu yalnız giriş bloğu
  ile bağlam satırı arasına (Karar 9).
- Dock'un fare yolu (Codebase 2) → `Drawn` bandın çizilen yüksekliği ve giriş
  satırı sayısını `px`/`fill_rows`'la aynı yazımda taşır;
  `window_point_dock` oradan okur, `rows: 1` kalkar.
- `e`→`u` boşluğu (Codebase 3, İşletme 2d) → ölçüldü (2026-09-24, zpty):
  zsh her `PS2` kabulünde `zle-line-finish` koşuyor, arada `precmd` yok. Karar
  11 eklendi.
- Yön kuralı ile bandın çukuru (Codebase 3) → `Motion::sync`'e `filled`'in
  kardeşi tek bit: "bandın hedefi bu karede değişti"; o karede yükselen içerik
  hedefi de süzülür, iki eğri birbirini götürür.
- `PS2` dayatması (Sadelik 2, İşletme ürün sorusu) → **kaldırıldı.**
  Mekanizmaya hizmet etmiyordu (taban `PREBUFFER` varken çıpadan, `BUFFER`'ın
  devam satırları 0. sütundan) ve bedeli kullanıcının `for>` bağlamını
  geçmişten silmekti; boşlukta kullanıcı tarafı.
- Izgara parametrizasyonu (Sadelik 3) → düzen fonksiyonu iki alan alır
  (ilk satırın başı, devam satırlarının başı); ızgara için ilk satırın başı
  imlecin ızgaradaki sütunundan gözlenir, devam 0; `PREBUFFER` ızgara
  hesabına girmez.
- `r` komutunun kapısı (İşletme 1, Codebase notu) → 031'in tam düzenleme
  kapısı (`dock_edit_line`: dock sahibi, ekleme keymap'i, ayna güncel nesle
  cevap, `w`), yapıştırmadan **önce** sorulur ve `r` aynı yazıma eklenir.
  Mekanizma cümlesi düzeltildi: diziyi önce `bracketed-paste-magic`'in kendi
  okuması yakalayıp geri itiyor; sonuç ölçümle.
- Sessiz kırılmalar (İşletme 2) → `last_ink` süzgeci `\n`'i atlar;
  `column_width('\n')` düzenin sorumluluğuna geçer; `blank_mirror` yeniden
  tanımlanır; hepsi dönüşüm phase'inde bekçiyle.
- Kapılar görmüyor (İşletme 3) → canlı zsh'i koşan `Session::spawn` deseniyle
  hermetik sınamalar (`for` döngüsü, çok satırlı yapıştırma, `r`); bant,
  ızgara orijini ve doldurma bandının kenarları **bileşim** olarak aynı karede
  sınanır; oh-my-zsh'li yapıştırma yalnız gözle kontrolde ve adıyla.
- Tavanın yeri (Codebase notu) → `DOCK_MAX_SHARE` `bt-gpu`'da (yerleşim
  kararı çizenin, `context_cols` emsali); `bt-core` bütçeyi `frame()`'e
  argüman olarak alır ve sarma genişliğini satır sayısıyla birlikte verir.
- Phase sırası (İşletme) → görünmez hazırlık phase'leri önce, `Multiline`'ın
  kalkması tek dönüşüm commit'inde (yarısı inseydi 2026-09-21 kusuru geri
  gelirdi).

**Reddedilenler:**
- Yeni duman jetonu (İşletme'nin kendi önerisiyle) — duman `/bin/sh` koşuyor,
  bant hep 0, jeton hiçbir şeyi kapılamaz; `kayma=`'nın anlamı da değişmez.
- Bandı ötelemeden türetmek (Sadelik'in kendi denediği) — `scroll_in` ve tek
  karede sıçrayan içerikte bandı sahte küçültür; ayrı animatör kalır.
- "Giriş satırları arasında çizgi yok", "üçlü tıklama mantıksal satır", "uzun
  tek satır sarılır" — jüriler ürün sorusu diye işaretledi; üçü de bariz
  kullanıcı beklentisi (tek editör yüzeyi, ızgaranın ve macOS'un kendi
  davranışı, komutun tamamı görünür) ve kullanıcının lehine okundu, gözle
  kontrolde bakılır.

## Karar 11: `PS2` satırları arasındaki `line-finish`

Ölçüm: `for …; do` ⏎ → `zle-line-finish` (`8133;e`) → `zle-line-init`
(`u`, `PREBUFFER` dolu); arada `precmd` yok, safha `Input` kalıyor. Bugün
`e` aynayı sıfırlayıp `Idle`'a indiriyor; çok satırlı dockta bu her ⏎'de
bandın küçülüp yeniden büyümesi ve `PREBUFFER` satırlarının bir an ızgarada
belirmesi demek.

**Öneri:** `e` aynanın **görüntüsünü** hemen silmez: safha `Input`'ta kaldığı
sürece son görüntü, bant boyu ve bastırma `HANDOVER_HOLD` kadar tutulur (caret
tutmasının aynı saati, ikinci bir süre yok); arada `u` gelirse yeni ayna
geçer, 133 `C` (komut koştu) ya da süre dolarsa bugünkü sıfırlama. Bekçisi
canlı zsh'le `for` döngüsü.

## Karar (2026-09-24, otonom akış)

Panelden geçmiş öneri; `/akis` altında kullanıcı onayı alınmadı. Yön (dock çok
satırı kendisi gösterir, yukarı büyür) kullanıcının ürün kararı.

- **Seçilen (Karar 1):** A — çizim tarafında büyüme; PTY `DOCK_ROWS`'la
  sabit, ızgara bandın ek yüksekliği kadar yukarı ötelenir, dolu ızgaranın
  tepesi geçici olarak kırpılır. B (PTY resize) nefes alan ekran ve zsh'in
  SIGWINCH yeniden çizimi yüzünden, C (örtme) komutun üstündeki çıktıyı
  kapattığı için reddedildi.
- **Karar 2:** `PREBUFFER` aynanın yedinci, isteğe bağlı gövdesi (yük
  bütçesine girer); dock'ta düzenlenebilir satırların üstünde, seçilebilir,
  salt okunur. `PS2`'ye dokunulmaz.
- **Karar 3:** sarma, tek satırlık uzun komut dahil; `window_skip` emekli.
- **Karar 4:** tavan `DOCK_MAX_SHARE` (ızgara satırlarının yarısı, tasarım
  sabiti, `bt-gpu`'da); aşınca caret'i izleyen dikey pencere.
- **Karar 5:** bandın ek satırı `Motion`'da kendi `Slide`'ında, iki yönde
  süzülür, `settled()`'e girer; Hareketi Azalt/`snap`/geometri snap'ler;
  birleştirme `set_origin`'de, öteleme `u16` kalır; bant değişen karede içerik
  hedefi de süzülür.
- **Karar 6:** 2B isabet testi; üçlü tıklama mantıksal satır, ⌘A bütün
  `BUFFER`; tel değişmez.
- **Karar 7:** tek düzen fonksiyonu (iki başlangıç alanı), bastırmanın
  `to`/`floor`'u ondan; satır sayısı `frame()`'de bastırmayla aynı okumada,
  `Cursor`'la çıkar ve `Session::dock`'a argüman gider.
- **Karar 8:** satır sonlu bracketed yapıştırmanın arkasına, 031'in tam
  düzenleme kapısı açıkken, `CSI 8133 ~ r BEL`; ölçülerek doğrulanır,
  tutmazsa güvenli yol kalır ve adıyla yazılır.
- **Karar 9:** `Control`, `Unavailable`, alternatif ekran değişmez; bağlam
  satırı altta; giriş satırları arasında çizgi yok.
- **Karar 10:** efekt anahtarı (satır, sütun); eşlenemeyen hâl `Reset`.
- **Karar 11:** `e` → `Input` safhasında görüntü `HANDOVER_HOLD` kadar
  tutulur.
- **Kullanıcı tarafına kapanan ürün soruları:** `PS2` dayatılmaz (kullanıcının
  `for>` bağlamı ızgarada kalır); uzun tek satırlık komut da sarılır ve dock'u
  büyütür; üçlü tıklama mantıksal satırı seçer; `PREBUFFER` seçilip
  kopyalanabilir; giriş satırları arasında çizgi yok; tavan pencerenin
  yarısı; bant iki yönde süzülür.
- **Reddedilen:** B, C; bandı ötelemeden türetmek; `PS2` dayatması; işaretli
  öteleme; yeni duman jetonu.

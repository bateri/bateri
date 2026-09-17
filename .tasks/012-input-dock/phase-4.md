# Phase 4 — Bastırma: giriş satırı ızgaradan çıkıyor

## Özet

Safha `Input` iken giriş satırının hücreleri ızgarada çizilmesin; phase-3'ün
bıraktığı çift görüntü kapansın.

_Requirements: R3.1, R3.2, R3.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()` döngüsünde
  `prompt_row..=cursor_row` aralığının hücreleri **glyph listesine girmez**.
  - **Aralık hesaplanabilir:** sıfır genişlikli `PS1` (phase-5) inmeden önce bu
    phase'te aralık prompt satırından imleç satırına; `prompt_row` OSC 133
    `B`'nin satırı. phase-5 sıfır genişliği getirince aralık **bütün
    sütunlar** olur.
  - **Safha kopyası `Term` kilidinden ÖNCE alınır** — `Theme` ile aynı örüntü.
    Safha `shell` yaprak kilidinde, sink `Term` kilidi altında ve ikisi hiçbir
    yerde iç içe girmiyor; kilit sırası bozulamaz.
  - **Çıpa taraması bastırmadan etkilenmez:** `cell.hyperlink()` okuması glyph
    üretiminden **bağımsız** koşar, yani blok şeridi yerinde kalır. Naif bir
    bastırma (satırı tümden atlamak) çıpayı da öldürürdü; bekçisi yazılır.
  - **Doluluk sayısı (`content_rows`) bastırılan satırları saymaz** — yoksa
    011'in tabana yapışması boş bir satır için yer ayırırdı.
- **`crates/bt-core/src/shell.rs`** — **özel kip tetiği.** ZLE'nin
  `bck-i-search`, `menu-select`, `zle -M` ve `CORRECT`'in `[nyae]`'i beş
  değişkenin **dışında** çiziyor; o anlarda bastırma **bırakılır** ve ızgara
  devralır.
  - Tetiğin sinyali bu phase'in asıl tasarım işi: ZLE tarafında kip ayırt
    edilebiliyor ve kanal zaten açık (phase-1), yani beşinci bir alan olarak
    taşınabilir. Alternatifi terminal tarafında sezgi olurdu — **seçilmez**,
    tahmin bu deponun yasakladığı sınıf.

## Kabul

- Yazarken metin **yalnız dock'ta** görünüyor; ızgarada giriş satırı boş.
- **Komut şeridi yerinde kalıyor** — bastırma çıpayı öldürmedi.
- Tab'a basınca tamamlama listesi ızgarada beliriyor ve kullanıcı ZLE'nin
  kendi arayüzünü eksiksiz görüyor. **Ölçüm bu maddeyi değiştirdi:** bastırma
  bırakılmıyor, çünkü bırakılmasına gerek yok — liste imleç satırının
  *altına* çiziliyor, yani aralığın dışında, ve ayna o sırada canlı kalıp
  seçili elemanı taşıyor. Gerekçe Uygulama Notları'nın ilk maddesinde.
- `bck-i-search` ve `CORRECT`'in `[nyae]`'i çalışıyor.
- İçerik tabana yaslanması bozulmuyor: bastırılan satır doluluğa sayılmıyor.
- Enter'dan sonra dock boşalıyor ve komut ızgarada normal bir satır olarak
  duruyor (bastırma yalnız `Input` safhasında).

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırı bastırmayı ve safha kopyasının kilit
  öncesi alındığını söyler.
- **Bilinen sınır:** `zle -I` ile basılan bir iş bildirimi çıpa satırını bir
  satır kaydırabiliyor (011 Karar 10a'nın kayıtlı sınırı). Bastırma aralığı
  çıpadan türediği için belirti burada **görünür** hâle geliyor: bir satır
  fazla ya da eksik bastırılabilir. Adıyla yazılır.
- **`psvar[9]` düşerse** bedeli artık şerit değil: prompt devredilmişken
  (phase-5) kimlik kaybı bastırma aralığını da belirsizleştirir. Borç
  `docs/YOL-HARITASI.md`'de, bu phase onu **büyütüyor** ve notu güncellenir.
- **Betik ve tel değişmedi:** özel kip tetiği ölçümle düştü (Uygulama
  Notları), yani `assets/shell/*` bu phase'de dokunulmadı ve OSC 8133'ün
  biçimi aynı kaldı. Üç kabuğun gözden geçirilmesi gereken bir şey yok; bash
  ve fish betikleri doğduğunda bastırma onlarda da `DockStatus` üzerinden
  çalışır, çünkü kapı kabuğa değil aynanın durumuna bakıyor.
- shader, ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.
- Ölçüm bekleyen iddia: yok. **Gözle kontrol bekliyor:** yeni prompt anında
  bir karelik satır sıçraması (Uygulama Notları'nın son maddesi).

## Uygulama Notları

- **Özel kip tetiği yazılmadı: ölçüm onu gereksiz kıldı.** Phase "beşinci bir
  alan olarak taşınabilir" diyordu ve bu phase'in asıl tasarım işi sayılmıştı;
  üç PTY probe'u (gerçek `zsh -i`, `pty.fork`) varsayımı çürüttü. ZLE'nin dört
  özel kipinin **hiçbirinde** ayna körleşmiyor:
  - `bck-i-search`: `line-pre-redraw` koşuyor (zsh'in `isearch-update`'inin
    hemen ardından) ve `BUFFER` **eşleşen geçmiş satırını**, `CURSOR` da
    eşleşmenin yerini taşıyor. `bck-i-search: …` istemi ham akışta
    `\r\r\nbck-i-search: _` ile **alt satıra** çiziliyor — yani bastırma
    aralığının (`çıpa..=imleç`) dışında ve görünür kalıyor.
  - `menu-select` (gerçekten devredeyken: `zmodload zsh/complist`,
    `menu select=1`): her harekette pre-redraw koşuyor ve `BUFFER` **seçili**
    elemanı taşıyor (`alfa_bir` → `alfa_dort` → `alfa_iki` gözlendi). Liste
    imleç satırının altında, yani görünür.
  - düz Tab listelemesi ve `zle -M`: aynı — ayna canlı, ek çizim altta.
  - `CORRECT`'in `[nyae]`'i: `line-finish` ondan **önce** koşuyor (logda
    doğrulandı), yani ayna zaten `Idle` ve bastırma kendiliğinden bırakılmış
    oluyor; ızgara devralıyor.

  Sonuç: bırakma sinyali **zaten açık olan kanaldan** geliyor (`DockStatus`),
  terminal tarafında sezgi yok, tel ve betik değişmedi. Karar 3b'nin "sinyali
  yok" gerekçesi de böylece kapandı — sinyal gerekmiyordu.
- **`$KEYMAP` denendi ve elendi.** İlk tasarım onu okuyacaktı; probe 1 isearch
  sırasında `KEYMAP=main` gösterdi (`isearch` değil). Hatırlamaya güvenilseydi
  tetik **sessizce ölü** doğardı — kip hiç yakalanmazdı ve belirti
  görünmezdi. `keymap-select` kancası çalışıyor (vi kipinde `vicmd`
  doğrulandı) ama menuselect'e hiç düşmüyor; ikisi de kullanılmadı.
- **Kabul'ün bir maddesi değişti** (yukarıda işaretli): "Tab'da bastırma
  bırakılmış" yerine "bırakılmıyor, gerekmiyor". Ürün tarafında daha iyi
  çıktı — giriş satırı her Tab'da ızgara ile dock arasında yer değiştirmiyor,
  hep dock'ta duruyor.
- **Döngüdeki sıra zorunlu ve değişti:** çıpa taraması → bastırma kapısı →
  `drawn_rows`. `drawn_rows` eskiden çıpanın **üstündeydi**; bastırılan satırı
  saymaması için altına indi. Çıpanın kapının üstünde kalması R3.2'nin
  kendisi.
- **`Cursor::visible` phase'in değişiklik listesinde yoktu, eklendi.** Caret
  dock'ta; ızgaranınki de çizilseydi kullanıcı iki caret görürdü ve
  ızgaradaki, altındaki harf bastırıldığı için **boş bir blok** olarak
  dururdu.
- **`content_rows`'a taban 1 kondu.** Dejenere hâlde (ilk prompt, üstünde hiç
  çıktı yok) bütün pencere bastırılıyor, `drawn_rows` sıfır kalıyor ve
  `Cursor::content_rows`'un `1..=rows` sözleşmesi bozuluyordu.
- **Sınamada `glyph_text` yetmedi ve ilk hâli boş yere yeşildi:** boşluk
  hücresi sink'e hiç girmediği için `"ls -la"` ızgarada dururken bile metin
  `"ls-la"` görünüyor, yani `!contains("ls -la")` **her hâlde** doğru. Satır
  bazlı `row_glyphs` yazıldı; iddia zaten satır bazlıydı.
- **Bilinen sınır — çok satırlı `BUFFER` ve bir karelik pencere.** Alt ucun
  aritmetiği `BUFFER`'daki satır sonunu (PS2, Esc-Enter) saymıyor; o hâlde
  gerçek satır sayısı hesaptan büyük ve kuyruk kısmen sızıyor. Yön güvenli
  (eksik bastırır), ama adıyla duruyor. Ayrıca `frame()` ile `Session::dock`
  yaprak kilidi ayrı ayrı alıyor: aralarına düşen bir `line-finish` **bir**
  kare boyunca "ızgara bastırılmış, dock boş" bırakabiliyor — kapatmanın yolu
  iki çağrıyı tek kilit turuna indirmek, yani `bt-gpu` sınırını değiştirmek.
- **phase-5'e devir — sıralama kısıtı, süs değil.** Bastırma aralığının üst
  ucu **çıpayı taşıyan hücreden** türüyor. phase-5 `PS1`'i sıfır genişliğe
  indirince `PS1` hiçbir hücre yazmıyor; `anchor_close` `PS1`'in sonunda
  kaldığı sürece kullanıcının yazdığı hücreler de çıpasız kalır,
  `suppress_from` hiç dolmaz ve **hem bastırma hem blok şeridi sessizce
  ölür** — üstelik üç kapı da yeşil kalır (duman `/bin/sh` koşuyor, testler
  çıpayı elle basıyor). Çare zaten planda (R4.2: `anchor_close` → `preexec`),
  ama bu, R4.1 ile R4.2'nin **aynı commit'te** inmesini zorunlu kılıyor:
  ayrı inerlerse aralarında kör bir hâl var.
- **Bilinen sınır — yeni prompt anında bir karelik sıçrama.** Bastırma
  devreye girince prompt satırı doluluktan düşüyor (`content_rows` bir
  azalıyor) ve 011'in tek yönlü kuralına göre daralma **snap**'liyor. `B`
  işareti ile aynanın `u` yükü aynı okuma parçasına düşerse ara durum hiç
  çizilmiyor; ayrı karelere düşerse bir karelik bir satır sıçraması görünür.
  Ekran doluyken (`content_rows == rows`) hiç doğmuyor. Gözle kontrol
  bekliyor; görünürse bu bir zevk kararı (satırı saymaya devam etmek uzun
  sarmalı komutta birkaç boş satır ayırırdı) ve kullanıcıya sorulur.

### `/code-review` (riskli phase) — 4 bulgu, 4 giderildi

- **Giderildi (orta, kod).** Aralığın **altı** imlecin satırıydı; ZLE caret'i
  tamponun içinde gezdirdiği için sarmalı bir satırda Ctrl-A ya da yukarı ok
  kuyruğu aşağıdaki satırlarda bırakıyordu — dock tamponun tamamını, ızgara
  kuyruğu gösteriyordu, yani phase'in kapatmaya geldiği çift görüntü **kalıcı
  olarak** geri geliyordu. Alt uç artık kesin veriden hesaplanıyor: caret'in
  sütunu ızgaradan, arkasındaki karakter sayısı aynadan
  (`SuppressedInput::chars_after_cursor`; uzunluk çözücüde **zaten
  sayılıyordu**, `DockState::display_chars` olarak saklandı, yani kare başına
  ek gezinti yok). Hata yönlü: `BUFFER`'da satır sonu ya da geniş glyph varsa
  **eksik** bastırır, fazla değil. Bekçisi
  `a_wrapped_input_line_is_suppressed_below_the_cursor_row_too` ve yük
  taşıdığı doğrulandı — eski sınırla `stuvwxyz0123` sızıyor.
- **Giderildi (yorum).** Kilit öncesi tek okumanın yorumu vermediği bir
  garantiyi iddia ediyordu ("dock'u boş bırakmak imkânsız"). `frame()` ile
  `Session::dock` yaprak kilidi **ayrı ayrı** alıyor; araya düşen bir
  `line-finish` bir karelik "ızgara bastırılmış, dock boş" doğurabiliyor.
  Yorum artık yalnız sağladığını söylüyor, kalanı bilinen sınır.
- **Giderildi (sınama).** `the_grid_takes_the_input_line_back_when_zle_lets_go`
  doğrudan `Idle` bekliyordu; `Idle` `DockStatus`'ün varsayılanı olduğu için
  `u` yükünü büsbütün düşüren bir regresyonda da yeşil kalırdı. Sınama artık
  önce `Live`'ı geçip bastırmayı orada doğruluyor, `e` sonra geliyor —
  gerçek sıra (yaz → Enter).
- **Giderildi (sınama).** `wait_mirror`'ın doc yorumu düzenleme artığı olarak
  `row_glyphs`'in üstünde kalmıştı.

## Checklist

- [x] `prompt_row..=cursor_row` glyph listesine girmiyor
- [x] Safha kopyası `Term` kilidinden önce alınıyor (`Theme` örüntüsü)
- [x] Çıpa taraması bastırmadan bağımsız; **bekçisi var**
      (`a_suppressed_input_line_keeps_the_block_stripe`)
- [x] `content_rows` bastırılan satırları saymıyor
- [x] Özel kip tetiği kanaldan geliyor (terminal tarafında sezgi **yok**) —
      tetik **yazılmadı**, ölçüm gereksiz kıldı; bırakma sinyali `DockStatus`
      (Uygulama Notları)
- [x] Test: bastırma açıkken şerit yerinde
- [x] Test: özel kipte bastırma bırakılıyor — iki kol ayrı ayrı:
      `the_grid_keeps_the_input_line_when_the_mirror_cannot_show_it`
      (`Unavailable`) ve `the_grid_takes_the_input_line_back_when_zle_lets_go`
      (`Idle`; `CORRECT`'in `[nyae]`'i ve Enter bu yoldan geçiyor)
- [ ] Gerçek zsh oturumunda gözle: Tab, Ctrl-R, `CORRECT` (+ `make duman`) —
      **kullanıcıda**, ajanın kabuğunda gerçek pencere yok
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`; `make kur`
      tetiklenmedi — `assets/shell/*`, `assets/bundle/*`, `crates/bateri` ve
      `kur` hedefi değişmedi)
- [x] `/code-review` (riskli phase: paylaşılan durum → `make test-yaris`)
- [x] Yayın etkisi yazıldı

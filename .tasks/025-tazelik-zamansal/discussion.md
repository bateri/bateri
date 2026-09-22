# Tazelik zamansal olsun — Tartışma

Seçenek biçimi: tek tasarım sorusu ("ayna güncel mi" nasıl sorulur), birden
çok yol. Hepsi aynı üç şeyi korumak zorunda:

- **K1** — `bracketed-paste-magic` hâlini yakalamaya devam etmek (ölçülmüş
  gerçek kusur: ayna bir tuş boyunca gelmiyor).
- **K2** — Yanlışın yönü **güvenli** kalmak: şüphede bastırmayı bırak, yani
  satır iki yerde görünsün ama sessizce kaybolmasın.
- **K3** — Kare yoluna O(n) iş **eklememek**; kapı `Term` kilidinin
  yakınında koşuyor.

## Seçenek A: Nesil sayacı — "ayna ızgaranın son yazımından sonra mı geldi"

Tarayıcı iki sayaç tutar: aynanın geldiği andaki nesil ve ızgaraya yazılan
son baytın nesli. Kapı `mirror_gen >= write_gen` sorar.

**Artıları:** emsali depoda (`observe_screen_clear`), tek karşılaştırma,
içerikten tamamen bağımsız — üç örneği (TAB, `^A`, `<hex>`) birden kapatır.

**Eksileri:** tuş vuruşu başına **normal** sıra "önce ayna, sonra redisplay"
(aşağıda ölçüldü), yani sağlıklı hâlde de `write_gen > mirror_gen` oluyor —
naif A `<hex>`'i değil **her tuşu** bozardı.

**Bu maddenin ilk yazımı olgusal olarak yanlıştı** ve panelin iki merceği
bağımsız olarak düzeltti: "alacritty satır başına yazım damgası vermiyor"
demiştim, oysa `alacritty_terminal` 0.26'da `Term::damage()` →
`TermDamage::Partial` + `LineDamageBounds` **var**. Gerçek engel başka ve
deponun kendisi zaten yazmış (`AdapterInner::dirty`'nin doc'u): `damage()`
**her çağrıda imleci koşulsuz kirletiyor** (`damage_cursor`), giriş satırı da
imlecin satırının ta kendisi — yani cevap her zaman "dokunuldu". Üstüne
`&mut self` + `reset_damage()` sahipliği ve kaydırma/resize sonrası
`TermDamage::Full`. Sonuç değişmiyor (A ölü), **gerekçe** düzeldi.

## Seçenek B: Tuş sayacı — "gönderdiğim tuşun aynası geldi mi"

`bt-shell` PTY'ye tuş yazıyor; `bt-core` o yazımı zaten görüyor
(`Session::send_input`). Kapı "son tuştan beri ayna geldi mi" sorar.

**Artıları:** sinyal **bizim** tarafımızda, kabuğa hiç sormuyor; bayat hâlin
tanımıyla birebir örtüşüyor ("ayna bir sonraki tuşa kadar gelmiyor").

**Eksileri:** her tuş ayna üretmiyor (komut koşarken hiç; `Idle`'da hiç), yani
ölçüt safhaya koşullu olmak zorunda. Ve tuşla ayna arasındaki pencere
**doğal olarak** açık: tuş gitti, kabuk henüz cevap vermedi — o aralıkta her
kare "bayat" der ve bastırma titrer. Bir tolerans (süre ya da tek tuşluk
gecikme) gerekir ve **tolerans ölçülmemiş bir sayıdır**.

## Seçenek C: Aynanın kendi sayacı — kabuk söylesin

Sarmalayıcı her aynaya monoton bir sayaç ekler (`line-pre-redraw` her
koşuşunda artar). Kapı "sayaç bir önceki kareden beri arttı mı" sorar.

**Artıları:** ayna ile kanca arasında hiçbir tahmin yok; sayaç tam olarak
"kanca koştu" demek. Betiğe tek alan, jeton sözleşmesi gibi **eklenir,
silinmez**.

**Eksileri:** "arttı mı" tek başına tazelik değil — ayna gelmediğinde sayaç
da artmıyor ve kapı yine içerikten bağımsız bir "bayat" sinyali **üretemiyor**;
yalnız "bu karede yeni ayna var" diyor. Bayatlık "ızgara değişti, ayna
değişmedi" demek ve ızgara tarafı hâlâ eksik. Üstelik bash/fish betikleri
doğduğunda alan üçünde de yazılmak zorunda.

## Seçenek D: Kapıyı kaldır, caret'i ayır

Tazelik kapısı yalnız **bastırmayı** sürsün; caret **ZLE satırı elinde
tuttuğu sürece** dock'ta kalsın (`Input` + `Live`).

**Artıları:** tek satırlık; kullanıcının gördüğü kusuru **bugün** kapatıyor
ve yeni bir sinyal istemiyor.

**Eksileri — ve bu ağır:** 012'nin kusurunu geri getiriyor. (Kayıt
düzeltmesi: o kusur **kullanıcıda gözlenmedi**, set kapısından geldi —
`012/teslim.md`'nin bulgular tablosu, 2. satır. İlk yazımı "ölçülmüş" diyordu
ve fazla güçlüydü.) Ayna
**gerçekten** bayatsa dock eski metni gösterir, ızgara yeniyi; caret dock'ta
kalırsa kullanıcının o an yazdığı satır caret'siz kalır ve caret bambaşka
bir metnin yanında durur. Yanlışın yönü **güvensiz** (K2).

## Karar Noktaları

1. **Ölçüm önce.** A'nın da B'nin de ölçütü "hangi yazım giriş satırına
   dokundu" sorusuna dayanıyor ve o soru **ölçülmedi**. Setin ilk işi
   seçenek seçmek değil, bir tuş vuruşunda ızgaraya **ne yazıldığını**
   ölçmek olmalı (saf pty, `zle -f`): kaç bayt, hangi satıra, aynaya göre
   hangi sırada. O ölçüm olmadan A ile B arasında seçim yapmak tahmin olur.
2. **Kapsam: üç örnek mi, biri mi?** `^A` bilinen sınır olarak duruyor ve
   kimse bildirmedi; `<hex>` bildirildi. Kök aynı olduğu için üçünü birden
   kapatmak **bedava** görünüyor, ama A/B'nin toleransı `^A`'da farklı
   davranabilir.
3. **Geri düşüş.** Yeni ölçüt yanılırsa ne oluyor? K2 gereği cevap
   "bastırmayı bırak" olmalı, yani yeni kapı da **içerik kapısının yanına**
   eklenir mi (ikisi birden geçmeli) yoksa onun **yerine** mi geçer? İkisi
   birden istemek `<hex>`'i kapatmaz — içerik kapısı zaten düşüyor.

## Ölçüm — tuş vuruşunun şekli (2026-09-22)

Karar Noktası 1'in istediği ölçüm. Saf pty, **gerçek sarmalayıcı** (dört
dotfile ile kurulmuş bir `ZDOTDIR`), `LANG=en_US.UTF-8`, `TERM=xterm-256color`.
Tuş yazıldıktan sonra gelen baytlar ayna (OSC 8133) ve ızgara parçalarına
ayrıldı:

| tuş | ayna | ızgara | sıra |
|---|---|---|---|
| `a` | 24 bayt | 1 bayt (`a`) | **ayna → ızgara** |
| `b` | 25 bayt | 3 bayt (`\b ab`) | **ayna → ızgara** |
| geri sil | 24 bayt | 5 bayt (`\b\ba \b`) | **ayna → ızgara** |
| `🥰` | 29 bayt | 21 bayt (`\ba ESC[7m<0001f970>ESC[27m`) | **ayna → ızgara** |

Dördünde de **tam bir** ayna ve sıra hiç değişmiyor.

### Ölçümün üç sonucu

1. **Seçenek A'nın naif hâli çürüdü.** Sağlıklı tuşta da aynadan **sonra**
   ızgara baytı geliyor, yani `mirror_gen >= write_gen` her karede "bayat"
   derdi. Ham nesil karşılaştırması ölçüt olamaz.
2. **Ayna ile redisplay bir çift ve ayna önce.** Yani her **durgun** anda
   ayna ızgarayı doğru tarif ediyor — bayatlık ancak "redisplay geldi, ayna
   gelmedi" hâlinde doğuyor ve o da tam olarak `bracketed-paste-magic`'in
   ölçülmüş şekli.
3. **Çift damga ölçütü kapatıyor** ve satır başına yazım damgası
   **gerekmiyor** (A'nın eksisi bu yüzden konusuz kalıyor):

```
stale  ⟺  mirror_at < key_at  ∧  any_write_at > key_at
```

- Sağlıklı: `key → mirror → write`, yani `mirror_at > key_at` → taze.
- Bayat: `key → write` (ayna yok), yani `mirror_at < key_at` **ve**
  `any_write_at > key_at` → bayat.
- **Uçuştaki pencere** (tuş gitti, kabuk henüz cevap vermedi):
  `mirror_at < key_at` ama `any_write_at < key_at` → **taze**, yani B'nin
  titreme eksisi de konusuz kalıyor. Ölçüm bunu mümkün kılan şey: ızgara
  aynadan **önce** yazılmıyor, dolayısıyla "yazıldı ama ayna yok" hâli
  yalnız gerçek bayatlıkta doğuyor.

Üç damganın ikisi tarayıcıda (ayna geldi, bayt uygulandı) ve biri giriş
yolunda (tuş yazıldı). İkisi `observe_screen_clear`'ın nesil sayacıyla aynı
yerde ve aynı kuralla okunuyor; üçüncüsü `Session::send_input`'ta.

**Bu bir ölçüm sonucu, seçilmiş bir yaklaşım değil** — panelin verdikti
üstüne gelecek.

## Muhakeme (2026-09-22)

Panel `/rfc` adım 6'nın pahalı karar sınıfından koştu: shell entegrasyonu +
dört yaklaşım.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de "bu iş yapılmasın" demiyor; üçü de **set bugünkü hâliyle `plan.md`'ye
geçemez** diyor. Üç mercek iki seçeneği bağımsız olarak elemiş ve iki olgusal
hatamı bulmuş.

**Kabul edilen itirazlar → plana girecek:**

- **C eleniyor — yetersiz değil *gereksiz*.** Tarayıcı `8133;u`'yu
  ayrıştırdığı anda "yeni ayna geldi"yi **zaten** biliyor
  (`TappedPty::read`'in `apply_scan` kapanışı); kabuktan sayaç istemek
  terminalin elindeki olayı tele koyup geri okumak olurdu. Üstüne tel biçimi
  göçü ve `make kur` yükü. *(Codebase-fit bir yanlış gerekçeyi de kapattı: C
  "üç kabuk" kuralından KIRMIZI **değil** — ayna yapısal olarak zsh-only ve
  emsali `KEYMAP`'in altıncı opsiyonel gövdesi.)*
- **D eleniyor ve gerekçesi düzeldi.** Codebase-fit yüklemi bölmeyi
  **denedi**: bastırma içerik kapısında, caret zaman kapısında kalırsa ya
  çizilen satır caret'siz kalıyor ya iki caret doğuyor — 012'nin iki kusuru.
  Kural delinmiyor: **satır nerede çiziliyorsa caret oraya gider.** Ayrıca D
  iki **yeşil** bekçiyi öldürmek zorunda (`a_stale_mirror_…`,
  `a_blank_mirror_below_the_anchor_is_stale`) ve bir seçeneğin K2'yi ihlal
  ettiğinin bundan ucuz erken uyarısı yok.
- **A eleniyor** (gerekçe yukarıda düzeltildi).
- **B, "yerine geçen" değil "gevşeten" olarak alınıyor.** Sadelik'in katkısı:
  `fresh = answered || içerik_kapısı`. Tolerans **gerekmiyor** ve B'nin
  reddedilme sebebi buydu — uçuştaki pencerede içerik kapısına düşüyoruz ve
  orada ızgara da ayna da eski, yani eşleşiyorlar. Saat kurulmuyor, dördüncü
  bir kare talebi doğmuyor, kare yoluna iş **eklenmiyor çıkıyor** (cevap
  gelmişse `last_ink_in_row`'un O(cols) taraması hiç koşmuyor).
- **Damga içeriğinin yanında durmak zorunda.** Codebase-fit'in en sert
  bulgusu: serbest bir bayrak `observe_screen_clear` emsalinin **yük taşıyan
  yarısını** bozuyor (o sayaç tek yazar / tek okur / **tek yön** —
  "kurmak için, düşürmek için değil"). Somut yarış: yaprak kilit turunda
  bayat ayna içeriği okunuyor, arada yeni ayna iniyor, `Term` turunda bayrak
  "cevap geldi" diyor → bastırma **eski** içerikle açık kalıyor, yani K2
  deliniyor. Çare ikinci bir kilit değil **yerleşim**: nesil `DockState`'e,
  içeriğin yanına; `suppressed_input` onu `caret` ile **aynı yaprak kilit
  turunda** taşıyor. Bayat okuma bayat damgayı da beraberinde getiriyor →
  yanlışın yönü güvenli.
- **`write_gen` gerekmiyor ve bu iki merceğin birleşiminden çıktı.**
  Codebase-fit haklı olarak "tarayıcının *yazım* diye bir kavramı yok" dedi
  (ground hızlı yolu baytları gezmiyor, `ESC[K` sıfır ground baytı üretiyor,
  aynanın kendi 24 baytı yazım sayılmamalı). Ama ölçüt **gevşetme** olduğu
  için "yazım oldu mu" sorusuna hiç ihtiyaç yok: cevap geldiyse taze,
  gelmediyse bugünkü kapı. Üç damga ikiye iniyor ve hızlı yola yeni makine
  girmiyor.
- **Kapının çıpa yarısı KALIYOR.** İşletme'nin yakaladığı yanlış ikilem:
  kapı tek değil iki yarım — mürekkep (`last_ink`) ve **geometri**
  (`at_anchor`). İkincisi yalnız boş aynada devreye giriyor, yalnız
  daraltıyor ve 012 phase-8'in ölçülmüş kusurunu (*satır gizli ama yer
  kaplıyor*) tutuyor. Değişen yalnız mürekkep yarısı.
- **Ölçüm phase olmayacak.** `proje.md` "ölçüm bir kapı değildir" diyor ve
  "ölçüm bekliyor" kalemi 2026-09-22'de emekli edildi. Sıra probu `/rfc`
  içinde koştu (yukarıda), sonucu `context.md`/`discussion.md`'ye **anlatı**
  olarak girdi, `docs/OLCUMLER.md`'ye girmiyor (orası sayının sahibi, sıranın
  değil) ve prob **depoda kalmıyor**.
- **Kapsam `^A`'yı vaat etmiyor.** Kimse bildirmedi, yönü güvenli ve
  `shell.rs`'te adıyla yazılı. Zamansal ölçüt onu bedavaya kapatıyorsa
  kapansın; set onun için genişletilmiyor.
- **Değişim tek commit.** Mürekkep teriminin gevşetilmesi ile zamansal terimin
  eklenmesi bölünürse arada `make hepsi` yeşil ama kullanıcıya **sıfır**
  değer veren bir phase doğar.

**Reddedilenler:**

- **İşletme'nin B eleştirisinin tolerans yarısı** — toleranslı B'yi
  hedefliyordu ve Sadelik'in gevşetme biçiminde tolerans **yok**. Saat de
  kurulmuyor, yani "tuş başına bir garanti uyandırma" ve `sessiz=` jetonunun
  koruduğu sınıf konusuz. İşletme'nin aynı bölümdeki **ikinci** gözlemi
  kabul edildi ve plana giriyor: tazelik kapısı `HANDOVER_HOLD`'un
  histerezisinin **dışında**, yani her yanlış verdikt **anında** bir caret
  sıçraması — bugünkü kusurun bu kadar göze batmasının sebebi de o.
- **Codebase-fit'in "atlas kutu çiziyor" itirazı** — alıntıladığı `CLAUDE.md`
  cümlesi (*"Emoji ve geniş glyph henüz yok … kutu çiziliyor"*) **artık
  dosyada yok**, 023 onu değiştirdi; dock emojiyi renkli çiziyor ve
  kullanıcının ekran görüntüsü bunu gösteriyor. İtirazın `^A` yarısı **kabul
  edildi** ve aşağıya karar noktası olarak geçti: kontrol karakteri dock'ta
  çizilmiyor (`dock.rs`), yani orada bastırma bilgiyi okunmaz kılar.

## Karar Noktası — kullanıcıya

`^A` gibi **dock'un çizmediği** bir karakterde bastırma açılırsa ızgaradaki
okunur `^A` gizlenir ve dock hiçbir şey göstermez. `<hex>`'te böyle değil:
dock emojiyi renkli çiziyor, yani orada bastırma bilgiyi **kazandırıyor**.
Mekanizma ikisini ayırt edemiyor.

**Öneri:** gevşetme alınsın, `^A`'nın çizilmemesi ayrı bir kalem olarak
kaydedilsin — 024 aritmetiği sütuna çevirdiği için `^X` yer tutucusu artık
**mümkün** (`dock::column_width`'in doc'u bunu yazıyor) ve doğru çare o.

## Karar (2026-09-23)

Kullanıcı `^A` sorusunu devretti ("durumunu bilmiyorum, danışmanı yap");
karar danışmanla birlikte verildi ve yukarıdaki öneri **alınmadı**.

**Öneri K2'yi deliyordu.** "Gevşetme alınsın, `^A` ayrı kalem" demek
`Ctrl-V Ctrl-A` yazan kullanıcıda sessiz kayıp üretir: cevap gelmiş sayılır,
ızgara bastırılır, dock kontrol karakterine glyph vermediği için o sütun boş
kalır ve kullanıcı yazdığını **hiçbir yerde** görmez. Bugün en azından
ızgarada görüyor. Deponun tek yasaklı yönü tam bu ve karar noktasının kendi
cümlesi ("orada bilgi kaybı olur") onu söylüyordu — onay istenecek bir şey
değil, dönüş işaretiydi.

**Karar 1 — Gösteremediğimiz satır ızgarada kalır; kural yeni değil.**
`Unavailable` ve `Multiline`'ın kuralının üçüncü uygulaması:

```
fresh = (answered && drawable) || (last_ink eşit && at_anchor)
```

`drawable` = görüntüde dock'un glyph **vermediği** bir kontrol karakteri yok.
Yüklem **tek fonksiyon** ve `dock::cell`'in kullandığının ta kendisi
(`column_width`'in yanında), yani iki tablo doğmuyor. `decode_line`'da
`last_ink`'in yanında hesaplanıyor ve `SuppressedInput`'ta aynı yaprak kilit
turunda taşınıyor — "damga içeriğin yanında" şartının aynısı.

| satır | `answered` | `drawable` | sonuç | bugün |
|---|---|---|---|---|
| `🥰` (zsh `<hex>` yazıyor) | ✓ | ✓ | **dock**, caret dock'ta | ızgara, caret sıçrıyor — **bu setin konusu** |
| `^A` (Ctrl-V Ctrl-A) | ✓ | ✗ | içerik kapısı → uyuşmaz → ızgara | aynı |
| Ctrl-V Tab | ✓ | ✗ | içerik kapısı → eşleşir (2026-09-18) → dock | aynı |
| `^A` + `🥰` aynı satırda | ✓ | ✗ | ızgara | aynı |
| yapıştırma, ayna gelmedi | ✗ | — | içerik kapısı | aynı |

`^X` yer tutucusu **ayrı kalem** (TAB kontrol ama `^I` değil boşluğa açılır,
DEL `^?` — kendi kararlarını istiyor). O kalem geldiği gün `drawable` kendi
kendine `true` döner ve bu daraltma söner; yol haritasına öyle yazılıyor.

**Karar 2 — Damganın etiketleme kuralı ve iki bilinen sınır.** Ayna
çözüldüğü anda `key_gen` okunur ve `DockState::answers`'a yazılır; kapı
`answers == key_gen` sorar. `key_gen` `send_input`'ta, `Msg::Input`
gönderilmeden **önce** artıyor — tersi olsaydı tuşun kendi aynası bir önceki
nesille damgalanabilirdi. Yanlışın yönü **çoğunlukla** güvenli (bayat okuma
bayat damga getirir → bugünkü kapı) ama koşulsuz değil ve iki pencere adıyla
yazılıyor:

- **Tuş + hemen yapıştırma.** k tuşunun aynası yoldayken k+1 bracketed
  yapıştırma olarak giderse ayna k, k+1'in nesliyle damgalanır; ızgara
  yapıştırmayı alır, ayna eski kalır, kapı "cevap geldi" der. Pencere ms
  mertebesinde ve bir sonraki tuşta kendini onarıyor. Terminalin "ZLE k+1'i
  işledi mi" sorusunu bilme yolu yok, yani sınır mekanizmanın özünde;
  çözmeye çalışıp seti şişirmiyoruz. Bekçisi sınırı adıyla tutuyor.
- **Kabuğun dışından yazım.** Arka plan işi giriş satırına yazarsa (`echo`
  bir `&` işinden) o baytlar tuş değil, `key_gen` oynamıyor ve kapı "cevap
  geldi" der: yazı düzenleme boyunca bastırılan satırda gizli kalır, Enter'da
  geçmişte görünür. Bugün içerik kapısı onu yakalıyordu. Ayırmanın yolu bir
  **yazım** nesli (`write_gen`) ve panel onu pahalı buldu (tarayıcının yazım
  kavramı yok, hızlı yol baytları gezmiyor). Sıklığı düşük, kayıp kalıcı değil;
  sınır olarak yazılıyor, kullanıcı bildirirse kendi kalemi.

**Karar 3 — Plan tek phase, tek commit** (panelin kararı): gevşetme ile
zamansal terim bölünürse arada kullanıcıya sıfır değer veren yeşil bir phase
doğar.

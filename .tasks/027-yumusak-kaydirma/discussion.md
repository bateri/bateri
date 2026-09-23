# Yumuşak kaydırma — Tartışma

Birbirine bağlı ama ayrı karar noktaları var; biçim karar-listesi. Karar 1
setin mimarisi ve seçenekleri gerçekten ayrışıyor; kalanlar onun üstünde
daha dar sorular.

Terimler: **görsel konum** `v` — pencerenin dipten kaç satır geriye baktığı,
`f32`, `v ≥ 0`, doldurma bandının sanal kaydırması içinde ölçülmüş (bandlı
pencerede `v = 0` dip, `v = 1` bir satır yukarısı; `scroll_locked`'ın
`virtual_before` eşlemesinin sürekli hâli). **Tam kısım** `⌊v⌋`
`display_offset`'e iner, **kesir** `v − ⌊v⌋` ekrana piksel olarak.

## Karar 1: Kaydırma konumunun sahibi ve çentiğin animasyonu

### Seçenek A — Tek kesirli konum; hedef `bt-core`'da, animasyon `bt-gpu`'da

Ötelemenin (011) örüntüsü: **hedefi** terminal semantiğini bilen taraf,
**konumu** animatör tutar.

- `bt-core` (`Session`) görsel hedefi tutar (`v*`). Tekerlek olayı onu
  `Term` kilidi altında, rota kararıyla **aynı turda** oynatır ve geçmişin
  iki ucuna kırpar; kırpma hedefi değiştirmediyse kare istenmez (bugünkü
  `wake_if_moved`'ın ikizi — momentum uçta olay yağdırmaya devam ediyor).
- `bt-gpu::motion` üçüncü bir tek eksenli animatör kazanır (konum `v`).
  Trackpad olayında konum **doğrudan** hedefe oturur (parmağı izleyen şey
  AppKit'in deltası, animasyon değil — momentum da AppKit'in
  `momentumPhase` olaylarıyla geliyor, yani kendi yavaşlatıcımız yok).
  Çentikte konum hedefe mevcut stille (`ease`/`spring`) süzülür — yeni sabit
  yok, `Slide`'ın fiziği.
- Link her içerik karesinden **önce** konumu `Session`'a yazar
  (`set_grid_top` emsali) ve `frame()` onu `Term` kilidi altında
  `display_offset`'e (bant eşlemesiyle) ve kesire çözer; kesir sınırdan
  geçer (`Cursor`), orijine eklenir.

**Artıları:**
- `display_offset` her karede **çizilen** satırlarla aynı, yani seçim, fare
  eşlemesi ve rapor koordinatı hiç ayrışmıyor; kesir de `Origin`'e girdiği
  için `point_to_cell` bedava doğru.
- Kesir her zaman tek yöne (aşağı öteleme) → ekrana fazladan **tek** satır
  lazım ve o satır ızgaranın **üstünde**: doldurma bandının kanalı
  (Karar 2).
- Animasyon `motion.settled()` kapısının içinde; Hareketi Azalt ve `snap`
  tek yerde (`Motion::origin_mode` emsali).

**Eksileri:**
- Çentiğin süzülmesi boyunca `⌊v⌋` değişen her kare bir **içerik karesi**
  (`frame()` koşar): liste korunamaz, çünkü görünen satırlar değişiyor.
  Bugünkü "hareket karesinde `Term` kilidine girilmez" kuralının dışında
  bir kare türü; gerekçesi saatin içerik tadının aynısı (animasyon aynı
  içeriği farklı çizmiyor, **içeriğin kendisini** değiştiriyor).
- `bt-gpu` `bt-core`'a kare başına bir sayı daha yazıyor — emsali var
  (`set_grid_top`), yön aynı.

### Seçenek B — Ofset anında oynar, hareket yalnız görsel (`scroll_in` örüntüsü)

Tekerlek `display_offset`'i bugünkü gibi tam çentik kadar anında oynatır;
öteleme ters yönde o kadar itilip hedefine süzülür (`Motion::scroll_in`'in
iki yönlü hâli). Trackpad'in kesri statik bir itme olarak orijine eklenir.

**Artıları:**
- `Term` bugünkü gibi olay anında güncel; kaydırma kareleri hareket karesi
  kalabiliyor (liste korunur, yalnız öteleme değişir).

**Eksileri:**
- Geçmişe doğru çentikte ızgara hedefinin **üstünden** başlar ve pencerenin
  **altında** çentik boyu kadar şerit açılır. Oraya çizilecek satırlar
  görünen pencerenin altındaki satırlar ve onları veren bir kanal yok —
  dördüncü bir bant (kendi listeleri, kendi viewport'u, kendi sayaç
  muafiyeti) doğar. Kanal gelmezse şerit ~150 ms boş kalır: "yarım
  çizilmiş" görüntü, bu deponun reddettiği şey.
- Olay anındaki `display_offset` ekrandakiyle ayrışıyor: süzülme boyunca
  fare eşlemesi `Term`'ün yeni ofsetine göre çeviriyor, göz eskisini
  görüyor — orijin terimi farkı kapatıyor ama yalnız ızgaranın içindeki
  satırlar için.
- Trackpad'in kesri ile çentiğin süzülmesi iki ayrı mekanizma.

### Seçenek C — `bt-shell`'de zamanlayıcı satır satır gönderir

Çentiği `NSTimer` ile birkaç tam satırlık adıma böler.

**Eksileri:** kesir yok (trackpad hâlâ basamaklı), link'in dışında ikinci
bir kare ritmi doğuyor (boşta sıfır karenin tek kapısı `motion.settled()`),
Hareketi Azalt ikinci bir yerde. Yalnız karşılaştırma için.

→ ✅ **A'nın bölüşümü, göreli modelle** — yukarıdaki A metni ilk önerinin mutlak `v*`'sini anlatıyor; panelden sonra mutlak konum kalktı (`## Muhakeme`, `## Karar`).

## Karar 2: Kesirli konumun çizimi

- **(a) `⌊v⌋` + aşağı öteleme** — ızgara `display_offset = ⌊v⌋`'la
  çizilir, orijine `+kesir` satır eklenir; tepede açılan şeridi ızgaranın
  hemen üstündeki **tek** satır kapatır ve o satır doldurma bandının
  kanalından geçer (bugün "ofset terimi yok" diyen bant döngüsü ofset
  terimini kazanır). Alt satır pencerenin altına ya da dock'un opak
  zeminine taşar ve kırpılır.
- **(b) `⌈v⌉` + yukarı öteleme** — fazladan satır **altta** gerekir ve
  onu verecek kanal yok (Karar 1 B'nin şeridi).

→ ✅ **(a)**, tepe satırı bandın kanalından ama kendi kapısı ve kendi sayısıyla (`## Karar`).

## Karar 3: Jest bittiğinde kesir

- **(a)** Konum en yakın tam satıra **süzülür** (trackpad jesti ya da
  momentumu bittiğinde).
- **(b)** Kesirde kalır.

(b)'de dinlenen pencerenin son satırı dock'un altında yarım durur, tepedeki
yarım satır seçilemez (bant satırı) ve doldurma bandının dibe yaslı
değişmezleri (`fill_shown`) kesirli bir dinlenme hâli görmek zorunda kalır.

→ ✅ **(a)**, göreli pay olarak; momentum başlarsa yerleşme biter.

## Karar 4: Ayar

Bölüm `[motion]` (bir animasyon ayarı; `cursor_motion` ve `reduce_motion`
orada). Adı ve değerleri: referansın adı (`scroll.smooth`) ve bu dosyanın
dizge-enum geleneği (`cursor_blink`, `osc52` — bool yok).

→ ✅ `[motion] smooth_scroll = "on" | "off"`, varsayılan `"on"`.

## Karar 5: Hareketi Azalt ve `cursor_motion = "snap"`

Kullanıcı isteği: ikisinde de bugünkü satır adımı. Soru mekanizma: ikinci
bir yol mu, aynı yolun nicemlenmesi mi.

→ ✅ nicemleme kaynakta: `bt-shell`'in tek `bool`'u, `false` kolu bugünkü yol.

## Karar 6: Rota ve tam satır yolları

Ok (alternatif ekran) ve rapor (fare kipi) kolları tam satır istiyor ve
kapsam dışı. Kesirli yol yalnız `WheelRoute::Scroll`'da; rota kararı
kesirden **önce** ve tek kilitte.

→ ✅ rota önce ve `bt-core`'da; olay kesirli ve tam satır hâliyle birlikte iner.

## Muhakeme (2026-09-23)

Önerilen: Karar 1 A (mutlak `v*` `bt-core`'da, mutlak konum `bt-gpu`'da,
`frame()` `⌊v⌋`'yi `display_offset`'e yazar), 2a, 3a, 4
`smooth_scroll`, 5 Motion'da nicemleme, 6 rota önce.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — A'nın biçimi doğru, durum yanlış yerde; mutlak konum bant eşlemesini ikinci kez yazdırıyor |
| Codebase-fit | SORUNLU — katman yönü temiz; `display_offset`'e ikinci yazıcı, dört dış yazıcıyı eziyor |
| İşletme | SORUNLU — aynı iki sahiplik; jest sonu sinyali ve rota/artık API'si planda yok |

**Kabul edilen itirazlar → plan değişikliği:**

- **`display_offset`'in dört dış yazıcısı var** (üç jüri de; girdide dibe
  dönüş `send_input` → `scroll_locked`, Shift+PgUp `scroll_page`, geçmişteyken
  gelen çıktıda alacritty'nin ofseti artırması, resize'ın `grow_lines`'ı).
  Her karede mutlak bir `⌊v⌋` yazan `frame()` dördünü de ezer: yazılan satır
  bir kare ya da süzülme boyunca görünmez, okunan satır okuyanın altından
  akar. → Mutlak konum **kalktı**; model **göreli** (Sadeliğin taslağı):
  tam satırın tek yetkilisi yine `scroll_locked`, `Session`'ın tek yeni
  durumu **kesir**, ve hem trackpad deltası hem animatörün kare başına
  teslim ettiği pay aynı `Session` çağrısından (delta) geçiyor. Dış yazıcı
  bir şey ezmiyor, çünkü kimse mutlak bir sayı yazmıyor. Bant eşlemesi
  (`virtual_before`, `1..=band`, resize muafiyeti) ikinci kez yazılmıyor ve
  `every_notch_moves_the_screen_by_one_row_at_most` olduğu gibi geçerli.
  Dibe dönüş ve Shift+PgUp kesri sıfırlıyor ve uçuştaki süzülmeyi
  bitiriyor (nesil; Karar 1).
- **Tepe satırının kapıları bandınkiler değil** (Sadelik, Codebase-fit):
  `fill_rows`/`slide_fill_rows` kaydırılmış pencerede kapalı, dock'suz
  pencerede `fill_rows` hiç koşmuyor ve Ctrl-L bayrağı kaydırılmış
  pencerede düşmüyor — Ctrl-L'den sonra trackpad'le yukarı çıkan kullanıcı
  her kesirde boş bir yarım satır görürdü. → Kanal paylaşılıyor (sink,
  viewport, fill-yerel satır, `point_to_cell` reddi), **kapı paylaşılmıyor**:
  tek koşul satırın defterde var olması. Sayısı `Cursor::fill`'e
  **karışmıyor** — karışsaydı `filled` biti (`link.rs`, `cursor.fill > 0`)
  kaydırılmış pencerede de açılır ve ötelemenin yön kuralını gevşetirdi — ve
  `fill_shown`'a yazılmıyor (uzantının bugünkü emsali).
- **Kare kapısı** (üç jüri): süzülme boyunca görünen satırlar değişiyor, yani
  o kare içerik karesi. Karar link'in **yerel** kararı ("kaydırma animatörü
  uçuşta → `frame()` koşar"), `Waker::wake`'ten istenmiyor (sözleşmenin
  yasağı). Kaba kural kabul (Sadelik): "yalnız `⌊v⌋` değişince" inceliği
  ölçülmemiş bir maliyete karşı erken optimizasyon.
- **`Motion::sync`'in ofset snap'i** (Codebase-fit): süzülme her tam satır
  geçişinde ofseti oynatıyor. Öteleme ve imleç bugünkü gibi snap'lesin
  (süreklilik kesirden geliyor, ötelemeden değil), ama bu snap üçüncü
  animatöre **dokunmuyor**.
- **Jest sonu sinyali** (İşletme): parmak kalkınca önce `phase == Ended`
  geliyor ve momentum gelip gelmeyeceği o olayda bilinmiyor. → Yerleşme
  `Ended`'da başlıyor ve **göreli**: kalan pay `round(kesir) − kesir`.
  Momentum başlarsa yerleşme bitiriliyor ve momentum deltaları oradan devam
  ediyor — göreli model olduğu için sıçrama yok, zamanlayıcı yok. Momentum
  bitince yeniden yerleşme.
- **`off` bugünkü yolun ta kendisi** (Sadelik, İşletme): Hareketi Azalt,
  `cursor_motion = "snap"` ve `smooth_scroll = "off"` `bt-shell`'de tek
  `bool`'a iniyor (Hareketi Azalt'ın bugünkü örüntüsü) ve `false` kolu
  bugünkü `wheel_lines` + artık yolu. Motion'a nicemleme kipi girmiyor; geri
  alma yolu bayt bayt bugünkü davranış ("yarıçap 0, hale 0" emsali).
- **Rota ve artık** (Codebase-fit, İşletme): rota `bt-core`'da seçiliyor,
  artık `bt-shell`'de. → `bt-shell` olayı tek çağrıda hem **kesirli**
  (kaydırma kolu için) hem **tam satır** (ok/rapor ve `off` için) hâliyle
  gönderiyor; artığın sıfırlanma kuralı kollara göre yeniden yazılıyor.
  Niyet sınıflaması (hassas mı, faz, momentum fazı) saf bir fonksiyon —
  `wheel_lines` emsali, hermetik sınanır.
- **Konumun geri yazımı** (Sadelik): `set_grid_top` gibi ayrı bir atomik
  gerekmiyor; animatörün payı `frame()`'den hemen önce bir `Session`
  çağrısıyla teslim ediliyor.

**Reddedilenler:**

- *Codebase-fit'in taslağı: mutlak `v` korunup `frame()`'in başında dış
  sapma (`display_offset − applied`) `v*`'ye ve animatöre eklensin* — doğru
  ama göreli model aynı dört yazıcıyı **hiç** sapma saymadan kapatıyor;
  uzlaştırma kuralı gerektirmeyen tasarım uzlaştırma kuralı olandan basit.
- *Sadeliğin ikinci yolu: `scroll_locked` silinsin, `v*` tek koordinat
  olsun* — ölçülerek düzeltilmiş bant kenarlarını yeniden yazdırıyor.
- *Yeni jeton* — önerilmedi zaten (İşletme): duman kaydırmıyor, jeton hep
  sıfır olur. `kayma=` anlamını koruyor, üçüncü animatör ona katılmıyor.
- *Ürün sorusu (Sadelik): `off` "animasyon yok, delta olduğu gibi" mi
  "tam satır" mı* — sorunun gözlemi doğru (Mos'un kendi yumuşattığı akışı
  tam satıra kesmek ona basamak geri getirir), ama talep `off`'u açıkça
  "bugünkü satır adımı" diye tanımlıyor ve geri alma yolu tam da o. Mos'un
  olaylarının hassas delta taşıyıp taşımadığı ölçülmedi; taşıyorsa `on`
  kolunda zaten doğrudan izleniyor (çentik süzülmesi yalnız hassas
  olmayan olayda). Kayda geçti, açılmadı.

## Karar (2026-09-23, otonom akış)

- **Seçilen (Karar 1):** A'nın bölüşümü, **göreli** modelle. Tam satırın tek
  yetkilisi `scroll_locked` (bant eşlemesi dahil), `Session`'ın tek yeni
  durumu `[0, 1)` aralığında bir kesir. Trackpad deltası ve süzülmenin kare
  başına payı aynı delta çağrısından geçiyor; kesir 1'i aşınca tam satır
  `scroll_locked`'a iniyor, uçta kesir sıfırlanıyor ve kare istenmiyor. Çentik
  süzülmesi `bt-gpu::motion`'da ikinci bir `Slide` örneği ("teslim
  edilecek kalan satır"; mevcut stilin fiziği, yeni sabit yok); istek
  `Session`'da birikiyor, link kare başında alıyor. Dibe dönüş ve Shift+PgUp
  kesri sıfırlayıp bir nesil artırıyor, nesil değişince süzülme bitiyor.
  Süzülme uçuştayken kare içerik karesi (link'in yerel kararı).
- **Seçilen (Karar 2):** (a) `⌊v⌋` + aşağı öteleme; tepe satırı doldurma
  bandının **kanalından**, kendi kapısıyla (satır defterde var mı) ve kendi
  sayısıyla (`fill`'e ve `fill_shown`'a karışmadan).
- **Seçilen (Karar 3):** (a) jest ya da momentum bitince en yakın satıra
  süzülme, göreli pay olarak; momentum başlarsa yerleşme biter.
- **Seçilen (Karar 4):** `[motion] smooth_scroll = "on" | "off"`,
  varsayılan `"on"`, kayıt anında.
- **Seçilen (Karar 5):** nicemleme **kaynakta**: `bt-shell` ayar + Hareketi
  Azalt + `cursor_motion = "snap"`'i tek `bool`'a indiriyor, `false` kolu
  bugünkü tam satır yolu.
- **Seçilen (Karar 6):** rota önce ve `bt-core`'da; olay kesirli ve tam
  satır hâliyle birlikte iniyor; kesir yalnız `WheelRoute::Scroll`'da.
- **Reddedilen:** B — geçmişe doğru çentikte pencerenin altında şerit açıyor
  ve onu kapatacak kanal yok; C — kesirsiz ve link dışında ikinci ritim;
  mutlak `v` (ilk öneri) — dış yazıcıları eziyor; 2(b) — altta satır ister;
  3(b) — dinlenen pencerede yarım satır; Motion'da nicemleme — `off`'u
  bugünkü koddan ayırıyor.

# Phase 2 — Sorular ve ayar

## Özet

⌘W, kırmızı düğme, ⇧⌘W ve ⌘Q koşan iş varken soruyor; `[terminal]
confirm_close` hangi durumda sorulacağını seçtiriyor.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.5, R2.6, R2.7, R2.8, R3.1, R3.2_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `ConfirmClose` enum'u (`Never`,
  `Running` varsayılan, `Always`), `Settings::confirm_close`; ayrıştırma
  öteki üç değerli anahtarların örüntüsüyle (`terminal.confirm_close`,
  tanınmayan değer kendi anahtarını korur). `terminal()` izdüşümüne ve
  `changes()`'e **girmez** (emsal `caret`). `TEMPLATE`, varsayılan anahtar
  listesi ve round-trip / tanınmayan değer sınamaları.
- **`docs/AYARLAR.md`** — şablon bloğu (sınama eşitliği zorluyor),
  `[terminal]` tablosu ve kısa açıklama: ne sayılır "koşan" (kabuğun
  dışında ön plandaki program; `ssh` dahil), ne sayılmaz (arka plan işi,
  kabuğun kendi döngüsü), `exit` hiç sormaz, sistem kapanışı da sorar.
- **`crates/bt-shell/src/window.rs`** —
  - `should_ask` (saf, sınanır): süreli koşu → hayır ilk satırda, sonra
    `confirm_close` ve — yalnız `running`'de — `foreground()`.
  - Soru kurucusu: pencere listesi alır, başlık/açıklama/düğmeleri üretir
    (metin Karar 4; metin üretimi saf ve sınanır, `NSAlert` kurulumu ince).
  - `windowShouldClose:`: süreli koşuda `true`; sayfa yuvası doluysa
    `false`; `should_ask` hayırsa `true`; evetse sayfayı açar, yuvaya koyar,
    `false` döner. Blok yalnız `id` yakalar ve yuvayı her yanıtta boşaltır; **yalnız**
    `NSAlertFirstButtonReturn` kapatır, başka her yanıt iptal — kabuk sayfa
    açıkken çıkarsa `close` sayfayı düşürür ve blok başka bir yanıtla gelir,
    `forget_window` de bir tur ertelendiği için `app.window(id)` o arada
    hâlâ bulunabilir. `close` bir ana kuyruk turu ertelenir
    (`windowWillClose:`'un örüntüsü): AppKit'in sayfa sökümünün içinde koşmaz.
  - `closeWindow:`: grubun sekmeleri için tek soru (key pencerede sayfa),
    onayda her sekmeye `close` — artık `performClose:` değil, yani sekme
    başına ikinci soru doğmaz.
  - **Ölçüm önce**: kırmızı düğme çok sekmeli pencerede bir sekmeyi mi
    grubu mu kapatıyor, sekme çubuğunun "Close Other Tabs"ı kaç
    `windowShouldClose:` doğuruyor. Sonucu Uygulama Notları'na; yalnız
    ölçülen kol yazılır. Grup kolu çıkarsa kural "bir jest, en çok bir
    soru": grubun bir sekmesinde sayfa açıkken grubun öteki sekmelerinin
    `windowShouldClose:`'u `false` döner ya da soru grubu kapsar — seçim
    ölçümün gösterdiği çağrı sırasına göre, gerekçesi notta.
- **`crates/bt-shell/src/app.rs`** — `applicationShouldTerminate:`: süreli
  koşuda `NSTerminateNow` ilk satırda (süreç tablosuna dokunmadan);
  pencere yoksa ya da `should_ask` bütün pencerelerde hayırsa
  `NSTerminateNow`; değilse uygulamayı öne alır, tek `NSAlert`'i `runModal`
  ile sorar (bütün pencerelerin koşan işleri, sekme sayısıyla), `Quit` →
  `NSTerminateNow`, `Cancel` → `NSTerminateCancel`. `will_terminate`'in
  "kapatma onayı yok" cümlesi ve `performSelector` satırındaki "block2 yok"
  notu güncellenir.
- **`crates/bt-shell/Cargo.toml`** (workspace manifest'i değişmez, üye
  bayrakları üyenin listesinde — `NSWindowTabGroup` emsali) —
  `objc2-app-kit` bayraklarına `NSAlert`, `NSButton`, `NSControl`,
  `block2`; `block2 = { workspace = true }` kenarı. Yorum: yeni crate değil,
  sürüm oynamıyor, `Cargo.lock`'ta iki listeye birer satır, karar kaydı
  `discussion.md` → Karar 2; "`block2` kenarı yok" cümlesi düzelir.
- **`CLAUDE.md`** — Sekmeler paragrafına tek cümle (koşan iş varken
  kapanış sorar, `exit` sormaz, işaretçi `.tasks/028-kapatma-onayi/`);
  `settings.toml` anahtar listesine `confirm_close`; katman tablosunda
  `bt-shell` satırına `block2` (sayfanın tamamlanma bloğu) ve `libc`'nin
  kullanımına `proc_*` (koşan işin tespiti); "Kapanış sınırlı bekler"
  maddesindeki ⌘Q cümlesi soruyla tamamlanır.

## Kabul

- `should_ask` sınamaları: süreli koşu her üç ayarda hayır **ve** tespit
  çağrılmıyor; `never` hayır; `always` evet; `running` koşan işe göre.
- Metin sınamaları: tek sekme / grup / çıkış, tek ad / çok ad / adsız,
  `always` kolunda koşan iş yokken.
- Ayar sınamaları: varsayılan, üç değer, tanınmayan değer anahtarını
  korur, tema yazımı `confirm_close`'u ve bilinmeyen anahtarı korur.
- `make hepsi` yeşil; `make duman` yeşil ve jeton satırı değişmemiş
  (süreli koşu hiçbir yoldan sormuyor); `make denetim`'in `Cargo.lock`
  uyarısı yalnız iki beklenen satır.
- Gözle kontrol (set kapısının devir mesajına): boşta ⌘W sessiz kapanır;
  `claude` açıkken ⌘W sayfa açar ve `claude` der, Esc iptal, Return kapatır;
  sayfa açıkken ikinci ⌘W yeni sayfa açmaz; sayfa açıkken `claude`'dan çıkıp
  `exit` yazmak pencereyi sayfasıyla kapatır; iki sekmeden birinde `vim`
  varken ⇧⌘W tek sayfa; ⌘Q tek uyarı ve iptal edilince hiçbir şey
  kapanmaz; bateri arkadayken Dock ▸ Quit uyarısı önde; `exit` sormaz;
  `sleep 100 &` sonrası ⌘W sormaz; `ssh` açıkken sorar; `confirm_close =
  "always"` boşta da sorar, `"never"` hiç sormaz — ikisi kaydedince, yeniden
  açmadan. Hücre yüzeyleri (ızgara, dock, bant) bu setin konusu değil:
  çizim değişmiyor.

## Checklist

- [x] Ölçüm: kırmızı düğme ve "Close Other Tabs" (Uygulama Notları)
- [x] `ConfirmClose` + şablon + `docs/AYARLAR.md`
- [x] `should_ask` + soru kurucusu
- [x] `windowShouldClose:`, sayfa yuvası, kimlik yakalayan blok
- [x] `closeWindow:` tek soru
- [x] `applicationShouldTerminate:`
- [x] Cargo bayrakları + kenar + yorumlar
- [x] Bayatlayan cümleler ve `CLAUDE.md` (`libc`'nin `proc_*` cümlesi phase-1'de girdi; kalan `block2`)
- [x] Test: `should_ask`, metin, ayar sınamaları
- [x] Doğrulama geçti (`make hepsi` + `make duman`)

## Uygulama Notları

- **Ölçüm (gerçek pencere, `make kur` paketi, 2026-09-23).** Kırmızı düğme
  çok sekmeli pencerede **grubu** kapatıyor: iki sekmede grubun her
  sekmesine birer `windowShouldClose:` (id 2, sonra 3; grup sırası, seçili
  olan sonda). Sekme çubuğunun "Close Other Tabs"ı seçili olmayan her
  sekmeye birer çağrı (id 1, sonra 0). Çağrıların hepsi aynı olay turunda.
- **Grup kolu: jest bir tur sonra ve kapsamıyla karar veriyor.**
  `windowShouldClose:` kendisi karar vermiyor; sekmeyi işaretliyor
  (`close_requested`) ve jestin ilk isteği turun sonuna tek iş kuruyor. İş
  işaretli sekmeleri topluyor ve **tek** soru soruyor: grubun tamamıysa
  "Close this window?", tek sekmeyse "Close this tab?", arasıysa
  "Close N tabs?" (`close_scope`, yeni bir `CloseScope::Tabs`). Boş sekmeler
  de kümede: iptal hiçbir şeyi kapatmıyor ve başlık kapanacak şeyi söylüyor.
  Sayfa grubun **seçili** sekmesinde (arka sekmedeki sayfa görünmüyor;
  "Close Other Tabs"ta kapanacakların hiçbiri seçili değil). Blok bu yüzden
  tek kimlik değil kimlik listesi yakalıyor — yine yalnız kimlik.
- **⌘W `performClose:` değil `closeTab:`** (plan `performClose:` diyordu;
  ölçüldü): kırmızı düğmenin grup kapanışı `false`'la durdurulduktan sonra
  AppKit sonraki `performClose:`'u da grubun her sekmesine yaydı (log: ⌘W
  id 0 ve 2'ye `windowShouldClose:`), yani ⌘W "Close this window?" sordu.
  Kendi eylemimiz kapsamı kendisi biliyor; sekme çubuğunun menüsü ve kırmızı
  düğme hâlâ `windowShouldClose:`'dan.
- **Esc elle bağlandı**: belgeye rağmen "Cancel" düğmesine Esc sayfayı
  gerçek pencerede kapatmadı (Return ilk düğmede çalıştı);
  `setKeyEquivalent("\u{1b}")`. Bağlamadan sonraki Esc gözle kontrole kaldı
  (ekran sürüşü yarıda kesildi).
- **`TerminalWindow::close` açık sayfayı önce `Cancel`'la kapatıyor**: kabuk
  sayfa açıkken çıkınca bloğun cevabı iptal ve pencere sayfasıyla gidiyor.
- **Metin**: grupta tek koşan sekme varsa sayı değil ad ("“vim” is still
  running."); `always`'in boş metni kapanacak oturumu söylüyor. Tırnaklar
  tipografik.
- **`foregrounds_to_ask`**: `running`'de tablo karar için bir kez okunuyor
  ve metin aynı okumayı kullanıyor; `always`'de karar tabloya bakmıyor, ad
  için soru kesinleşince okunuyor.
- **⌘Q'nun ayar ödüncü `runModal`'dan önce bırakılıyor**: modal döngü run
  loop'u döndürüyor ve o sırada gelen kayıt açık ödünçle panikle biterdi.
- **Set kapısı düzeltmeleri** (`/code-review`): turun sonundaki iş isteyen
  pencereyi değil dikili bayrakları arıyor (ilk isteyen aynı turda
  kapanınca öteki sekmelerin bayrağı kalıcı kalıyordu); `never` ertelemeden
  AppKit'in kendi kapanışına bırakıyor; tek hedef arka sekmeyse (× düğmesi)
  o sekme seçilip soru onda açılıyor; ⌘W terminal olmayan pencerede (About)
  app delegate'in `closeTab:`'ından `performClose:`'a düşüyor. Bilinen
  sınır: "Close Other Tabs" sorusu açıkken sayfayı taşıyan seçili sekmenin
  kabuğu çıkarsa jest düşüyor, öteki sekmeler açık kalıyor.

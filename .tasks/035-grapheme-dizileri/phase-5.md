# Phase 5 — Kümeleme açık; sözleşme güncel

## Özet

Kümeleme varsayılan açılır ve `CLAUDE.md` ile yol haritası bugünkü
sözleşmeye çevrilir.

_Requirements: R5_

## Değişiklikler

- **`crates/bt-core/src/session.rs` / `crates/bt-shell/src/app.rs`** —
  oturum seçeneğinin varsayılanı açık; süreli koşu dahil bütün pencereler.
  Bayrak kod yolunda kalır (geri alma tek satır), ayar anahtarı **değil** —
  kullanıcıya açılmıyor (Karar 2).
- **`CLAUDE.md`** — 023 paragrafının "grapheme dizileri kapsam dışı"
  cümlesi, 024'ün "sıfır genişlikli kod noktasını atlıyor" cümlesi, sınır
  `Cell` ve katman tablosu (`bt-core`: okuyucu döngünün sahibi; `bt-atlas`:
  küme şekillendirme) kural + tek cümle gerekçe + işaretçi olarak; iki
  ürün bedeli (`discussion.md` → Karar) adıyla.
- **`docs/YOL-HARITASI.md`** — 024 kapanışındaki "grapheme dizileri taban
  karakteriyle çiziliyor (ayrı set)" cümlesi kapanır, kapsam dışı kalanlar
  (çıplak 78, tek RI, emoji dışı kümeler, aramanın kümeyi görmemesi)
  sete bağlanmamış borç olarak.

## Kabul

- `make duman` yeşil.
- Gözle kontrol (devir mesajı): Claude Code'da `🇹🇷 "YouTube için…"`
  satırı — bayrak tek renkli glyph, tırnak iki sütun sonra; ızgarada
  `echo '👍🏽 ❤️ 👨‍👩‍👧 🌡️ x'` — dördü de tek glyph, `x` hizalı; doldurma
  bandında aynı satır (Tab listesi kalkınca); dock'ta aynı dizileri
  **yazarak** — satır dock'ta kalıyor, caret doğru sütunda, ⌫ (düzenleme
  kapısı açıkken, yani ekleme keymap'inde) bütün kümeyi siliyor.

## Checklist

- [x] Varsayılan açık
- [x] `CLAUDE.md` ve yol haritası
- [x] `Atlas::slot`'un Left→Whole kısayolu `slot != TOFU`'yu düzlemsiz soruyor (023 kodu; renk düzleminin 0. yuvası TOFU sanılır — phase-1'in `cluster_as_base`'te düzelttiği karışıklığın ikizi): düzlem koşulunu ekle, bekçi yaz ← phase-1 ARTIK
- [x] Basılı ⌫/⌦/←/→ kümeyi yine bölebiliyor: küme komutu nesli ilerletiyor, ayna cevap verene kadar kapı kapalı ve araya düşen tekrar tuşu bugünkü yoldan ZLE'ye kod noktası olarak gidiyor (`🇹🇷🇺🇸`'de ikinci ⌫ yalnız `🇷`'yi silebilir). Kapı kapalıyken son aynaya göre tüketmek/kuyruklamak bir tuş davranışı kararı — varsayılan açılmadan önce seç ya da bilinen sınır olarak yaz ← phase-4 `/code-review`
- [x] Doğrulama geçti (`make hepsi` + `make duman`)

## Uygulama Notları

- **Varsayılan tek satırda açık** (`bt-shell` `window::start_session`,
  süreli koşu da oradan geçiyor); `SessionOptions` `Default` taşımıyor,
  sınamaların seçeneği (`test_options`) kapalı kalıyor ve kümeyi isteyen
  sınama onu açıkça açıyor.
- **Basılı tuş kümeyi bölmüyor — bilinen sınır olarak bırakılmadı
  (orkestratör kararı).** Kapı ayna yoldayken artık son düzenleme komutunun
  **beklenen sonucuna** bakıyor (`shell::DockPrediction`: `BUFFER`, caret,
  gönderimin nesli; `send_input` nesli döndürüyor). Kuyruk yerine tahmin:
  komutun etkisini biz tanımlıyoruz (`[S,E)` silinir, caret `S`), ZLE
  komutları sırayla uyguluyor ve yeni tel ya da widget gerekmiyor. **Yanlış
  tahminin güvenli olduğu cümle:** tahmin ancak widget komutu reddettiyse
  ya da kabuğun dışından bir yazım geldiyse yanlış ve ikisi de `BUFFER`'ın
  uzunluğunu değiştiriyor — sonraki komutun `L`'si tutmuyor, widget hiçbir
  şey yapmıyor: bedel kayıp bir tekrar, bölünmüş bir küme değil. Başka
  girdi (yazı, yapıştırma) nesli ilerletip tahmini düşürüyor; araya
  fareyle kurulan seçim de (bayat aynanın metnine karşı kuruldu) kapıyı
  kapatıyor.
- **Sapma — phase-4'ün "tek kod noktası bugünkü yol" kuralı daraldı:**
  satırda bir **emoji** kümesi (birden çok kod noktalı, iki sütunlu) varsa
  tek kod noktalı komşu da komutla gidiyor. Ölçüt emoji, birleştirici değil
  (`/code-review`): NFD bir yol (`Masaüstü`) satırı kümeli yapıp düz tuşları
  ZLE'nin bağlamalarından (autopair) koparırdı; birleştiricili komşu yine
  phase-4'teki gibi bütün gidiyor. Olmasaydı `🇹🇷ab`'de basılı ⌫ `b`'yi ZLE'ye gönderip
  zinciri kırar ve aynadan hızlı üçüncü tekrar `🇷`'ye inerdi. Kümesiz
  satır bayt bayt bugünkü yolda; satırın uçları da (sondaki → zsh-
  autosuggestions'ın öneri kabulü, baştaki ←/⌫, sondaki ⌦) — uçta
  bölünecek bir şey yok. Yazım efektleri etkilenmiyor: `dock::diff`
  aynadan aynaya, tuşun yolundan bağımsız.
- **Kalan dar pencere (basılı tuş değil):** yazı ya da yapıştırmanın hemen
  ardından, ayna gelmeden basılan ilk düzenleme tuşu — farklı tuşlar arası
  geçiş, tekrar değil — bayat aynada kapalı kapıdan ZLE'ye gidiyor.
- **Sınamalar `od` üstünde:** kabuk hiç ayna basmıyor, yani ilk komuttan
  sonra ayna kalıcı olarak bayat — tekrarın aynadan hızlı geldiği an.
  `a_held_backspace_never_splits_a_cluster` (`🇹🇷a🇺🇸`'de üç ⌫ → üç komut,
  baytlarıyla) iki mutasyonla kırmızı gösterildi: tahmin kolu silinince ve
  kümeli satırda tek kod noktalı yönlendirme kalkınca;
  `other_input_ends_the_prediction` ilkinde.
- **`Atlas::slot`'un kısayolu** ret kaydını `(TOFU, Plane::Mask)` bütünüyle
  tanıyor; bekçi (`color_slot_zero_answers_the_left_request`) iç tabloyu
  kuruyor — tek hücreye sığan renkli glyph bugünkü fontlarda yok; eski
  `slot != TOFU` ile kırmızı gösterildi.
- **Set kapısı `/code-review` (beş bulgu):** (1) DECAWM kapalıyken son
  sütunda genişleyemeyen hücre kümenin eski kalanını ikinci kez alıyordu
  (`❤‍🔥` → çift ZWJ) — `handler::widen` hücrede kalanı sayıp atlıyor,
  sınamalı; (3) yukarıdaki emoji ölçütü, sınamalı
  (`only_an_emoji_line_routes_single_code_points`); (4) tahmin satırında
  ⇧←/⇧→ sessizce yutuluyordu — tahminde bugünkü yolundan gidiyor
  (`DockEditLine::fresh`), sınamalı; (5) atlasın küme interner'ı tavansızdı
  — tavanı negatif önbelleğinki, ötesi taban karakter, sınamalı. Dördünün
  kırmızısı mutasyonla gösterildi.
- **Waive — (2) dock'ta küme `BUFFER`/öneri sınırını aşabiliyor.**
  `layout_with` akışı (`PREDISPLAY ++ BUFFER ++ POSTDISPLAY`) tek parça
  kümeliyor: geçmişte `echo 👍🏽` varken `echo 👍` yazılınca öneri `🏽` ile
  başlıyor, küme sınırı aşıyor ve caret iki sütun solda, glyph `BUFFER`'ın
  renginde. Izgara aynı satırı kümelemiyor (arada SGR var, sarmalayıcı
  kümeyi kapatıyor). Çaresi akışa bir "dikiş" indeksi: `Walk`, `layout`,
  `grid_span` ve farkın bütün çağıranları — setin sonunda orantısız ve
  senaryo dar (tek başına yazılmış bir değiştirici ya da ZWJ'le biten
  `BUFFER` artı onunla başlayan bir öneri). Yol haritasının 035 borç
  kalemine yazıldı.
- **Set kapısı `/audit`:** `make denetim` temiz; tek bulgu `layout_with`'in
  gerekçesiz `#[allow(clippy::too_many_arguments)]`'ı (phase-3), gerekçesi
  yazıldı. `bt-shell`'in ara sıra çöken iki sınaması (phase-1 pano SIGSEGV,
  phase-3 "foreign exception" SIGABRT) bu phase'in üç `make hepsi` koşusunda
  görülmedi; pano sınaması setin koduna dokunmuyor, sete bağlandığına dair
  kanıt yok.

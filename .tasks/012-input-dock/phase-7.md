# Phase 7 — Alternatif ekranda dock kalkıyor

## Özet

vim, htop gibi alternatif ekrana geçen uygulamalar pencereyi **tamamen** geri
alsın; dock kalksın ve çıkışta insin.

_Requirements: R5.2, R5.3, R6.1_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — alternatif ekran bayrağı sınırı geçer.
  Bugün `Term` kilidi altında okunuyor ama `pub` değil; `Cursor`'a değil
  **kendi sorgusuna** eklenir (`Cursor` kare kaydı, bu bir oturum gerçeği).
- **`crates/bt-shell/src/app.rs`** — geçiş görülünce ızgara yüksekliği
  değişir ve `Session::resize` çağrılır.
  - **Resize render yolundan çağrılmaz.** Kare yolu bayrağı görür ama
    `Session::resize` `Term` kilidini alıyor ve o kilit okuyucunun ayrıştırma
    lease'inin arkasında bekleyebilir — "render yolu bloklanmaz" kuralı. Çağrı
    `dispatch2` ana kuyruğundan **bir sonraki turda** koşar; emsali
    `child_exit` ve OSC 52'nin pano işi.
  - **Bedel komut başına değil, geçiş başına.** `git log`, `man`, pager gibi
    her gün koşan komutlar resize **ödemiyor**; yalnız alternatif ekrana giren
    uygulamalar ödüyor ve orada zaten tam yeniden çizim oluyor.
  - Öteleme geçişte zaten snap'liyor (011'in yön kuralı), yani imleç ve içerik
    sıçraması **ek bir maliyet değil**.
- **Kapı kararı (R6.1).** Prompt ve dock yolunu bugün hiçbir kapı görmüyor:
  `smoke_shell` `/bin/sh`, `bateri.zsh`'i koşturan sınama phase-2'de doğdu ama
  gerçek pencere yok, `make kur` betiği `cmp`'liyor. Bu phase **seçimi yazılı
  yapar**:
  - ya **üçüncü bir yük** (`smoke_shell`/`load_shell` yanına, dock tanığı) —
    `smoke_shell`'e ikinci yük **eklenemez**, çünkü `hucre=8 glif=6 kural=15`
    sözleşmesinin tek sahibi o;
  - ya da **"prompt yolu kapısızdır"** diye açıkça kabul ve elle koşu +
    `/measure` ile tutulur.
  - Üçüncü yük seçilirse `IDLE_FRAME_LIMIT`/`QUIET_FLOOR` yeniden türetilir mi
    sorusu açılır ve türetme **ayrı commit**'le iner (`proje.md`).

## Kabul

- `vim` açınca dock kalkıyor ve vim pencerenin **tamamını** alıyor; çıkınca
  dock iniyor.
- `htop`, `less`, `man` aynı davranıyor.
- Geçişte içerik sıçramıyor (öteleme snap'liyor) ve vim'in ilk çizimi
  bozulmuyor.
- `git log`, `ls` gibi alternatif ekran kullanmayan komutlarda **hiç resize
  yok** — dock yerinde kalıyor.
- Resize ana kuyruktan koşuyor; render yolunda `Term` kilidi beklenmiyor
  (`make test-yaris` yeşil).
- Dock kapalıyken (entegrasyonsuz oturum) bu yol hiç çalışmıyor.

## Yayın Etkisi

- **Riskli phase:** paylaşılan duruma ve kilit sırasına dokunuyor →
  `make test-yaris` **ve** phase sonunda `/code-review`.
- **`CLAUDE.md`:** "Bugünkü hâl" dock'un alternatif ekranda kalktığını söyler.
- **Kapı sözleşmesi:** R6.1'in seçimi burada **yazılı** olur. Üçüncü yük
  seçilirse jeton **eklenerek** girer (`jeton silinmez, eklenir`) ve
  `IDLE_FRAME_LIMIT`/`QUIET_FLOOR` türetmesi ayrı commit.
- **`docs/OLCUMLER.md`:** yeni yük eklenirse `## Boşta kare`'nin bağlı girdi
  listesi büyür (bugün dört).
- shader, ayar şeması, terminfo, tema, app bundle: yok. Yeni bağımlılık: yok.

## Uygulama Notları

- **"Render yolu bloklanmaz" gerekçesi düzeltildi.** Bu bölüm resize'ın
  ertelenmesini `Term` kilidine bağlamıştı; ölçüm başka söylüyor: display
  link callback'i **ana thread'de** koşuyor (`link.rs` modül başlığı) ve
  `frame()` zaten her içerik karesinde `Term` kilidini alıyor, yani ikinci bir
  kilit turu yeni bir sınıf risk değil. Erteleme yine de zorunlu ve **gerçek
  sebebi başka**: `sync_geometry` drawable ölçüsünü, ızgarayı ve
  `DisplayLink` yerleşimini değiştiriyor; bunu tam da o kareyi encode eden
  callback'in içinde yapmak çizilen karenin altını oymak olurdu. Kuyruk bir
  thread geçişi için değil, **bir tur** ertelemek için.
- **Bayrağı `frame()` yayınlıyor, sorgu okumuyor.** `Session::alt_screen()`
  kendi sorgusu (plan böyle diyor, `Cursor`'a eklenmedi) ama `Term` kilidini
  **almıyor**: değeri `frame()` kilit altındayken bir `AtomicBool`'a yazıyor.
  İki kazanç: kare başına ikinci kilit turu yok ve okunan değer tam da o
  karenin `content_rows`'uyla tutarlı — dock'un payı ondan düşüldüğü için bu
  tutarlılık şart. Bayat kalamıyor, çünkü `?1049h`/`l` baytları da her okuma
  turu gibi `Wakeup` doğuruyor (phase-6'da doğrulandı).
- **Haberci enjekte ediliyor** (`bt_core::Wake` emsali): `bt-gpu` `bt-shell`'i
  göremez. Kapanış **yük taşımıyor** — koştuğunda gerçeği yeniden okuyor, yani
  birbirini kovalayan iki geçiş (vim aç-kapa) bayat bir değerle davranamıyor —
  ve **hiçbir şey yakalamıyor**, yani `DisplayLink` ile delegate arasında
  referans çemberi açılmıyor (link delegate'in ivar'ında). Ana kuyruğa
  `notify_settings_changed`'in yolundan varıyor: hedefsiz eylem, responder
  zinciri.
- **Dock'suz pencerede yol yapısal olarak kapalı:** haberci `None`, yani
  nöbet hiç kurulmuyor. Bir `if` ile kapatılsaydı "entegrasyonsuz oturumda yol
  hiç çalışmıyor" iddiası bir dalın doğruluğuna bağlı kalırdı.
- **Doğum değeri ayrı bir alan** (`dock_rows_at_birth`). Tek bir `dock_rows`
  üstüne yazılsaydı geri getirilecek değer `DOCK_ROWS` sabitinden kurulurdu ve
  entegrasyonsuz bir pencere alternatif ekrandan çıkarken dock **kazanırdı**.
  Karar `dock_rows_for(alt, birth)` saf fonksiyonunda ve sınaması dört kolu da
  tutuyor.
- **Değişmediyse hiçbir şey:** alıcı payı karşılaştırıp eşitse dönüyor. "Geçiş
  başına bir resize" (R5.3) iddiasını tutan kapı bu; `git log` gibi komutlar
  zaten bayrağı hiç oynatmıyor, yani haberci de koşmuyor.
- **Girişteki iki boyalı kare bilinen ve kısa:** bayrak okuyucu thread'de
  dönüyor, bir kare alt ekranı eski yükseklikte (dock hâlâ çizili) gösteriyor,
  sonra blok koşup resize ediyor ve SIGWINCH uygulamayı tam yüksekliğe yeniden
  çizdiriyor. "İlk çizimi bozulmuyor" kabulü **durulmuş** hâl için.
- **R6.1 kararı (kullanıcı):** prompt/dock yolu **kapısızdır** ve bu yazılı
  kabul. Üçüncü yük eklenmedi; tanığı elle koşu ve `/measure`. Gerekçe bedel:
  yeni bir yük yeni bir jeton, `IDLE_FRAME_LIMIT`/`QUIET_FLOOR`'un yeniden
  türetilmesi (ayrı commit) ve `docs/OLCUMLER.md`'nin bağlı girdi listesinin
  büyümesi demekti — kapanan bir set için orantısız. `make duman` bu yüzden
  dock'u **hâlâ görmüyor** ve `hucre=8 glif=6 kural=15` sözleşmesi
  dokunulmadan duruyor.

- **Gerçek pencerede çıkan kusur: boş prompt'ta iki imleç.** Kullanıcının
  ekran görüntüsü dock'un caret'inin yanında ızgarada da bir imleç bloğu
  gösterdi. Sebep zincirin başındaydı: bastırmanın **üst ucu** çıpadan
  (`suppress_from`) geliyor, çıpayı da prompt'un hücreleri taşıyor — ama
  sıfır genişlikli `PS1` hiçbir hücre yazmıyor ve kullanıcı henüz bir şey
  yazmadığı için ZLE de yazmıyor. Yani **çıpayı taşıyan hücre yok**,
  `suppress_from` `None` ve hücre kapısıyla birlikte imleç kapısı da
  açılmıyordu. İlk tuşta hücre doğduğu için kusur en sık görülen hâlde —
  boşta bekleyen prompt'ta — duruyordu.
  - Çare **kapıları ayırmak**: hücreler hangi satırların atlanacağını bilmek
    zorunda, yani çıpaya bağlı kalıyor; caret'in yeri ise bir satır aralığı
    sorusu değil — dock satırın sahibiyse caret dock'ta. `caret_in_dock`
    yalnız `suppress_to`'ya bakıyor ve o, tazelik kapısını zaten taşıyor
    (bayat aynada `None`, yani imleç ızgarada kalıyor).
  - Sınaması `an_empty_prompt_keeps_its_caret_in_the_dock_alone` ve prompt'u
    **hücresiz** kuruyor — `anchored_prompt` `$ ` bastığı için kusuru hiç
    gösteremezdi. Düzeltme geri alınınca kırmızı düştüğü doğrulandı.
- **`/code-review` üç bulgu getirdi, üçü de düzeltildi:**
  - *Sığmayan dal sessizce kırpılıyordu.* `render_context`'in son `take`'i
    dalı işaretsiz kesiyordu: `release/2.1` on iki sütunda `release` görünürdü
    — yani **var olmayan bir dal**. Aynı dosyanın doc'u bunu adıyla yasaklıyor
    ve işaret koymak (`rele…`) da çare değil, çünkü kısalmış bir dal adı zaten
    yanlış okunabilir. Dal artık **sığmıyorsa hiç çizilmiyor** ve genişliğin
    tamamı yola kalıyor (yolun kısaltması işaretli, yani yanlış okunamaz).
  - *`split_into_grid`'in doc'u phase-7 ile çelişiyordu* ("pay koşu boyunca
    oynamıyor"). Payın artık geçiş başına oynadığı ve bedelin komut başına
    olmadığı aynı commit'te yazıldı.
  - *Habercinin düşmesi hâlinde kurtarma abartılmıştı*: düşen bir **çıkış**
    bildirimi payı `0`'da bırakıyor ve sıradaki **giriş** aynı `0`'ı
    hesaplayıp no-op diyor, yani dock ancak tam bir tur sonra geri gelir.
    Bugün ulaşılamaz bir dal (hedefsiz eylem app delegate'e her zaman varıyor);
    yorum düzeltildi.

## Checklist

- [x] Alternatif ekran bayrağı sınırı geçiyor (kendi sorgusu, `Cursor` değil)
- [x] Geçişte dock kalkıyor/iniyor; ızgara yüksekliği değişiyor
- [x] `Session::resize` **ana kuyruktan**, render yolundan değil
- [x] Alternatif ekran kullanmayan komutta resize yok (bayrak oynamıyor →
      haberci hiç koşmuyor; üstüne alıcının "değişmediyse dön" kapısı)
- [x] Kapı kararı **yazılı**: "kapısızdır" kabulü (kullanıcı kararı; gerekçe
      Uygulama Notları'nda)
- [x] Test: geçişte resize bir kez, doğru yönde
      (`the_alternate_screen_takes_the_dock_and_gives_it_back` payın yönünü,
      `the_alternate_screen_flag_crosses_the_boundary_with_the_frame` bayrağın
      gerçek PTY'den geldiğini tutuyor)
- [x] Test: entegrasyonsuz oturumda yol hiç çalışmıyor (`dock_rows_for`'un
      `birth = 0` kolları; üstüne haberci o pencerede `None`)
- [ ] Gerçek pencerede gözle: vim, htop, less gir/çık — **kullanıcıda**
- [ ] Doğrulama: `make hepsi` ✅ · `make test-yaris` ✅ · `make duman`
      **kullanıcıda**
- [x] Riskli phase: `/code-review` koştu, üç bulgu da giderildi (yukarıda)
- [x] Yayın etkisi yazıldı

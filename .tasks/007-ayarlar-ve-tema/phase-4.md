# Phase 4 — Canlı izleme

## Özet

Ayar ve tema dosyalarını izle; kayıt anında yeniden oku, öncekiyle
karşılaştır ve yalnız değişeni uygula.

_Requirements: R5, R1.1, R1.2, R10_

## Değişiklikler

- **`crates/bt-shell/src/watch.rs` (yeni)** — `dispatch2` vnode kaynakları:
  - **Dizin kaynakları** (`WRITE`): `~/.config/bateri/` ve varsa `themes/` —
    dosya doğumu, silinmesi, yeniden adlandırılması (editörün "yeni dosya
    yaz + rename" kaydı).
  - **Dosya kaynakları** (`WRITE | EXTEND | DELETE | RENAME`):
    `settings.toml` ve **etkin kullanıcı teması** (gömülü tema seçiliyse
    yok). Yerinde yazmayı (`>>`, nano) ve symlink hedefindeki kaydı bunlar
    görür: dosya `std` ile salt okunur açılır, açılış symlink'i izler.
  - **Her olayda hepsi yeniden kurulur:** tüm kaynaklar iptal → yeniden oku →
    uygula → kaynakları yeniden kur. Birleştirme ayrı mekanizma değil (fark
    yoksa uygulanacak şey yok); taşınmış/yeniden yaratılmış dizinin bayat
    tanıtıcısı da böyle düşer.
  - Dizin yoksa kaynak kurulmaz; yeniden kurmayı dışarıdan tetikleyen bir
    giriş açık kalır (phase-6'nın "Ayarlar…"ı kullanır).
  - Olay işleyicisi **fonksiyon işaretçili** varyantla kurulur: `bt-shell`'e
    `block2` kenarı eklenmez. İşleyicinin kuyruğu parametre: üretimde ana
    kuyruk, sınamada özel seri kuyruk.
  - `bt-shell`'in `libc` kullanımı bekçinin dışına genişlemez.
- **`crates/bt-core/src/settings.rs`** — saf fark: (önceki, yeni) → neyin
  değiştiği (tema seçimi, `light_theme`/`dark_theme`, `scrollback`, …).
- **`crates/bt-core/src/session.rs`** — terminal seçenekleri canlı:
  alacritty `Config`'ini kuran **tek** fonksiyon hem `spawn`'da hem canlı
  değişimde kullanılır ve `Config`'i **bizim seçeneklerimizin tamamından**
  kurar. `Term::set_options` `Config`'in hepsini değiştiriyor (alacritty
  `term/mod.rs:499-516`): `..Config::default()` örüntüsüyle çağrılsa ilgisiz
  bir değişim geçmişi geri dönülmez kırpardı (`grid/mod.rs:154-158`).
  `Session` güncel seçenekleri tutar; değişimden sonra kare isteği (geçmiş
  küçülünce kaydırma ofseti de değişir). Doc: `set_options` başlık olayını
  `Term` kilidi altında yolluyor, `Adapter`'ın o kolu kilit almamaya devam
  etmeli.
- **`crates/bt-shell/src/app.rs`** — uygulayıcı (ana thread):
  - Ayrıştırılamayan dosya → yalnız ayar yuvası dolar, **hiçbir şey
    uygulanmaz**.
  - Değilse fark: tema seçimi ya da etkin tema dosyası değişti → ad çözümü →
    `set_theme` (görünüm uygulayıcısıyla aynı yol); `scrollback` →
    seçenekler. Yuvalar kendi kaynaklarına göre dolar/boşalır.
  - **Hata kuralı görünüm değişiminden farklı (R3.4):** kullanılamayan tema
    (yarım kaydedilmiş tema dosyası) takas edilmez, ekrandaki tema kalır ve
    tema yuvası dolar. phase-3'ün `AppDelegate::choose_theme`'i gömülü yedeğe
    düşüyor (`ThemeLoaded::or_embedded`) — bu yolda o yedek düzenleme
    sırasında pencereyi gömülü temaya çakar; `Failed`'i ayrı ele al.
  - Açılışta izleme kurulur; **hermetik dal** süreli koşuda kurmaz.
- **`docs/AYARLAR.md`** — "yeniden açılışta" cümlesi gider: kayıt anında
  uygulanır; yarım kayıtta ekran olduğu gibi kalır; dizini ilk kez kabuktan
  elle oluşturan kullanıcı için "Ayarlar…" ya da yeniden açılış (phase-6
  gelene kadar yalnız yeniden açılış).

## Kabul

Geçici dizinde, özel kuyrukla, zaman aşımlı bekleyişle (döngüyle yoklama
yok) izleme sınamaları:

- Yerinde ekleme (`append`) olay üretir.
- Yeni dosya yazıp üstüne yeniden adlandırma olay üretir; ikinci kayıt da
  (yeniden kurulumdan sonra) olay üretir.
- Symlink'li `settings.toml`: hedef dosyaya yazmak olay üretir.
- Dizin silinip yeniden yaratılınca kaynaklar yeni dizine kurulur.
- Olmayan dizin: kaynak yok, hata yok.

Ayrıca:

- Fark fonksiyonu: değişmeyen dosya boş fark; yalnız `scrollback` değişimi.
- `Config` kurucusu: bir seçenek değişince öteki korunur (bugün
  `scrollback`; phase-8 `osc52`'yi ekleyince aynı sınama genişler).
- `make test-yaris` yeşil (`set_options` `Term` kilidini ana thread'den alıyor).
- `make duman` jetonları değişmez.
- Göz: tema adını editörde değiştirip kaydetmek pencereyi anında değiştirir;
  aktif kullanıcı temasının bir rengini değiştirmek de; tırnağı eksik bir
  kayıt ekranı bozmaz, alt başlık hatayı söyler, düzeltince kaybolur.

## Yayın Etkisi

- **ayar şeması** — yeni anahtar yok; davranış değişti: ayar ve etkin tema
  dosyası **kayıt anında** uygulanır. Kayıt anında kullanılamayan dosya
  (geçersiz TOML, okunamayan, bir an yok olan) hiçbir şeyi değiştirmez;
  kabul edilmeyen değer anahtarı değiştirmez; kullanılamayan tema takas
  edilmez. Silinen `settings.toml` yeniden açılışa
  kadar etkisiz. `scrollback` küçültmesi geçmişi hemen siler. Uygulama
  açıkken ilk kez yaratılan `~/.config/bateri/` izlenmez (yeniden açılış;
  phase-6'nın "Ayarlar…"ı kapatır). `docs/AYARLAR.md` → Dosyanın yeri, Hata
  olursa (yeni "kayıt anında" tablosu), `[terminal]`.
- **`bt-core` pub API** — `TerminalOptions`, `Changes`, `Settings::terminal`,
  `Settings::changes`, `Settings::parse_keeping`,
  `Session::set_terminal_options`;
  `SessionOptions.scrollback` → `SessionOptions.terminal`.
- `CLAUDE.md` bugünkü hâl, katman tablosu (`dispatch2`: vnode kaynakları),
  "Ayarlar" maddesi; `bt-shell` `lib.rs` başlığı — güncellendi.
- Yeni bağımlılık yok, `Cargo.lock` değişmedi; `bt-shell/Cargo.toml`'da
  yalnız `dispatch2` yorumu (denetimin "Cargo.toml farklı" uyarısı bundan).
- Göz kontrolü bekliyor: bateri **içinde** vim açıkken tema değişimi (bu
  oturumda pencereye tuş gönderilemedi; dosya tarafı vim'le sınandı).

## Checklist

- [x] `watch.rs`: dizin + dosya kaynakları, her olayda yeniden kurulum
- [x] Saf fark fonksiyonu
- [x] `Config`'i tamamından kuran tek fonksiyon, canlı seçenek değişimi,
      kare isteği
- [x] Uygulayıcı: ayrıştırılamayan dosyada uygulama yok; hermetik dal
- [x] Test: dört izleme senaryosu + olmayan dizin, fark, `Config` koruması
- [x] `docs/AYARLAR.md`
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Sıra "önce kur, sonra oku"** (plan: iptal → oku → uygula → kur). Plan
  sırasında okumayla kurulum arasına düşen kayıt hiç olay doğurmuyor ve
  ekranda eski içerik kalıyordu. Yenisi eskisi düşmeden kuruluyor
  (`RefCell::replace`), arada boşluk yok; bedeli en çok bir fazladan olay.
- **İki yuva:** `config_watch` (kök, `themes/`, `settings.toml`) ve
  `theme_watch` (etkin tema dosyası). Görünüm değişimi de tema kaynağını
  yeniliyor (`choose_theme`): ad `theme_for(dark)`'tan türüyor ve eski adın
  kaynağı yeni dosyadaki yerinde yazmayı görmezdi. Plan bunu söylemiyordu.
- **Dizin maskesi `WRITE | DELETE | RENAME`** (plan: `WRITE`): dizinin
  kendisi silinince/taşınınca da olay gelsin, bayat tanıtıcı düşsün.
- **Canlı yolda `Missing` ve `Unreadable` de "hiçbir şey uygulanmaz"**
  (`Loaded::live`; plan yalnız ayrıştırılamayanı sayıyordu). Editörün "eskiyi
  kenara taşı, yenisini yaz" kaydında yol bir an yok; varsayılanları
  uygulamak her kayıtta pencereyi çakardı. Bedeli: silinen dosyanın
  varsayılanları yeniden açılışta.
- **Tema her olayda yeniden çözülüyor**, ayar dosyası bozukken de (son iyi
  ayarların adıyla); `Changes`'ta tema alanı yok (plan: fark tema seçimini de
  kapsıyordu). Etkin tema dosyası da bir kaynak ve hangi kaynağın haber
  verdiği bilinmiyor; tema alanı yarım bir ikinci kapı olurdu, aynı temanın
  takası zaten no-op. Kullanılamayan tema kuralı (`ThemeLoaded::or_current`,
  ekrandaki kalır) adın ayar dosyasında değişmesinde de aynı.
- **`Session` seçenekleri saklamıyor** (plan: "güncel seçenekleri tutar").
  Tek sahibi `Ivars.settings`, kapı `Settings::changes`; `set_terminal_options`
  her çağrıda `Config`'i tam `TerminalOptions`'tan kuruyor
  (`term_config`), ikinci kopya ayrışabilirdi.
- **"Dizin silinip yeniden yaratılınca" kabulü daraltıldı:** Karar 2 üst
  dizini izlemiyor, yani yeniden yaratılan dizini kendiliğinden hiçbir şey
  görmüyor. Sınama silmenin olay ürettiğini, silinmiş dizine kaynak
  kurulmadığını ve **dış tetikli** yeniden kurulumun yeni dizini gördüğünü
  sınıyor (phase-6'nın tetiğinin ilk sınaması).
- **Bildirim hedefsiz eylemle** (`settingsDidChange:`, phase-3'ün görünüm
  yolu): kaynağın context'inde delegate referansı yok, ömrü libdispatch'in
  iptal zamanlamasına bağlanmıyor.
- **`TempRoot`** `bt-shell/settings.rs`'te `#[cfg(test)]` modül düzeyine
  çıktı: izleme sınamaları da kullanıyor.
- **Test-first:** `watch` gövdesi boşken dört izleme sınaması, `changes` ve
  `set_terminal_options` boşken ikisi düştü; `term_config` sınaması
  kurucuyla birlikte yazıldı (bekçi).
- **Göz kontrolü** geçici `HOME` + `screencapture -l` ile, sabit tema
  `bateri`'den: `mv` ile üstüne taşınan kayıt `paper`'a geçirdi; **yeni**
  dosyaya `>>` ile yarım satır ekranı bozmadı, alt başlık satırı söyledi,
  düzeltilen kayıt (aynı kayıtta `scrollback = 50`) alt başlığı temizledi;
  etkin tema dosyasının rengini değiştirmek anında boyadı, bozmak ekrandaki
  temayı tuttu ("keeping the current theme"); vim'in kaydı temayı döndürdü;
  `themes/ink.toml`'un doğumu adı bekleyen ayarı uyguladı; `settings.toml`'u
  silmek hiçbir şeyi değiştirmedi. vim bu makinede `backupcopy=no`'da da
  inode'u korudu, yani **kenara taşıma dansı ve arada boş dosyanın
  yakalanması (bir karelik flaş) gözlenmedi** — birleştirme yok kararının
  olası bedeli, kayıtlı.
- **`/code-review` (high), iki bulgu, ikisi de düzeltildi.**
  - *Orta:* kayıt anında kabul edilmeyen `scrollback` (`"100000"` metin
    olarak) varsayılana düşüp geçmişi **geri dönülmez** kırpıyordu. Çare
    `Settings::parse_keeping` + `settings::load_keeping`: kayıt anında kabul
    edilmeyen değer (ve yanlış türdeki bölümün anahtarları) geçerli ayardan
    geliyor, tanı da o değeri söylüyor; dosyadan silinen anahtar yine
    varsayılan, tavanı aşan yine tavan. Kural bütün anahtarlara uygulandı,
    yalnız `scrollback`'e değil (`theme = 3` de adı değiştirmiyor). Kalan
    sınır `AYARLAR.md`'de: yazarken kaydeden editörün **geçerli** ara değeri
    (`100000` → `1`) de bir kayıt.
  - *Düşük:* yazmadan boşaltma (`: >`, `truncate`) kqueue'da yalnız `ATTRIB`
    veriyordu; dosya maskesine eklendi. Okumanın kendisi olay doğursaydı "kur →
    oku → olay" sonsuz döngü olurdu: `reading_does_not_notify` bunu bağlıyor,
    canlı koşuda boşaltmadan sonra süreç boşta kaldı (üç saniyede 0,04 sn
    CPU). Temiz bulunanlar (bulgu yok): iptal/context ömrü, hedefsiz eylem,
    kilit altındaki başlık olayı, `Config` kurucusu.
  - *Düşük düzeltmenin yan etkisi:* `ATTRIB` yerinde kaydın boşaltma anını da
    olay yaptı; arada okunan **boş** dosya geçerli bir TOML ve varsayılan
    `scrollback`'le geçmişi kırpabilirdi (göz kontrolündeki `: >` temayı
    varsayılana döndürmüştü, aynı yol). Çare dar: `load_keeping` boş (yalnız
    boşluk) dosyayı `Missing` sayıyor; açılışta ikisi zaten aynıydı. Daha geniş
    çare ("dosyada olmayan anahtar da geçerli değeri tutar") **reddedildi**:
    satırı silen ya da yoruma alan kullanıcı kaydettiği anda varsayılanı
    bekliyor (R1.1), küçük ayar dosyası tek `write` ile yazıldığı için yarım
    geçerli ara hâl gerçekçi değil. Tema dosyası aynı yolu almadı: boş ara
    dosya `bateri` renklerini bir an gösterir, geri dönülür ve açılışta boş
    `themes/x.toml`'u "bulunamadı" demek yanlış olurdu.

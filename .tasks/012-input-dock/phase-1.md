# Phase 1 — Dock kanalı: ikinci OSC kolu

## Özet

Tarayıcı OSC 133'ün yanında ikinci bir numarayı daha tanısın, base64 gövdeyi
çözsün ve sınır aşımını **görünür** bir sonuçla bildirsin.

_Requirements: R1.2, R1.3_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `Scanner` bugün numarayı `133`'e
  kilitliyor ve başkasını tampona hiç uğratmadan eliyor; ikinci kol açılır.
  - **Numara seçimi bir karardır:** `docs/ARASTIRMA.md`'nin desteklenen dizi
    listesiyle (0, 7, 8, 9, 12, 52, 104, 133, 777) ve yaygın terminal
    kullanımıyla çakışmayan bir numara seçilir; seçim gerekçesiyle doc'a
    yazılır. Başka terminalde tanımsız davranış üretmemeli.
  - **Kendi sınırı.** `PAYLOAD_LIMIT` (256) 133 için doğru ama bir komut satırı
    onu rahat aşar. Dock kolu kendi sınırını taşır; sayı **türetilir** (tipik
    satır uzunluğu × base64 şişmesi) ve doc'ta gerekçelenir.
  - **Aşım görünür.** Bugünkü `Skip` kolu çağırana hiçbir sinyal vermiyor;
    dock kolunda aşım bir **sonuç** olarak döner ki tüketici "gösteremiyorum"
    diyebilsin. Sessiz düşüş bu deponun yasakladığı belirti sınıfı.
  - Base64 çözme elle yazılır — yeni bağımsız bir crate **mimari karardır**
    (`proje.md`), bu phase onu açmaz.
- **`crates/bt-core/src/shell.rs`** — `DockState`: çözülmüş görüntü durumu
  (metin, caret sütunu, renk aralıkları, öneri kuyruğu). Yaprak kilit altında,
  `ShellLog`'un yanında. **Yeniden kullanılan tampon**, kare başına allocation
  değil — ölçüt `CLAUDE.md`'nin kare başına maliyet kuralı.
- **`crates/bt-core/src/session.rs`** — `Session::dock_state()`, `shell_state()`
  ile aynı örüntü: yaprak kilidi alıp bırakır, `Term` kilidine dokunmaz.

Bu phase'te **hiçbir şey çizilmiyor**; kanal açılıyor ve depolanıyor.

## Kabul

- Tarayıcı iki numarayı da tanıyor; 133'ün davranışı **birebir** değişmedi
  (mevcut sınamalar dokunulmadan geçer).
- Base64 gövde çözülüyor; bozuk gövde **panik değil** yoksayma üretiyor
  (`CLAUDE.md` → PTY ve ayrıştırma yolunda panik yok).
- Sınır aşımı çağırana görünür bir sonuç veriyor; sınama onu çiviliyor.
- Çıplak `ESC`'in diziyi bitirmesi kuralı dock kolunda da geçerli.
- `make test-yaris` yeşil: okuma yolu ile kare yolu arasında yeni bir
  paylaşılan durum var ve kilit sırası (`term` → `shell`) bozulmuyor.

## Yayın Etkisi

- **Riskli phase:** okuma yolu ve paylaşılan durum değişiyor →
  `make test-yaris` **ve** phase sonunda `/code-review` (`proje.md` → Kalite
  kapısı).
- **`CLAUDE.md`:** `bt-core` satırı tarayıcının artık yalnız 133 olmadığını
  söylemeli.
- Yeni bağımlılık: **yok** (base64 elle). Ayar şeması, tema, app bundle,
  terminfo: yok. Shader: yok.
- **Ölçüm bekliyor: tuş başına maliyet.** Tel biçimi bu phase'de donduğu için
  maliyetin *şeklini* burası belirliyor ve iki terimi var: kabuk tarafında saf
  zsh base64 kodlaması (baskın), ayrıştırıcı tarafında `vte`'nin `unhandled`
  dizgisi (yük uzunluğuyla orantılı, okuyucu thread'inde). Sahibi R6.2 ve
  **aracı da borç** (`BT_INPUT_LATENCY_SAMPLES` yok), yani `/measure` bugün
  kapatamaz. Sayı yazılmadı.
- `CLAUDE.md`'nin `bt-core` satırı güncellendi: tarayıcı artık iki kollu.

## Uygulama Notları

- **Numara `8133`.** Dışlama listesi hatırlanmadı, grep'lendi: ayrıştırıcımızın
  (`vte-0.15.0/src/ansi.rs`, `osc_dispatch`) yorumladığı numaralar 0, 2, 4, 8,
  10–12, 22, 50, 52, 104, 110–112; üstüne sahipli numaralar (7, 9, 133, 633,
  777, 1337, 9278, 30001–30002). Dört hane bilinçli: numara tuş **başına**
  akışa giriyor. Gerekçenin tamamı `DOCK_OSC`'un doc'unda — `docs/ARASTIRMA.md`
  Metalterm'in envanteri, bizim kararımızın yeri değil.
- **Biçim bu phase'de donduğu için kodlayıcı önce yoklandı.** Saf zsh
  (`nomultibyte` + `${(s::)}` + aritmetik) doğru base64 üretiyor — UTF-8 ve
  dolgu dahil — ve fork'lu yoldan belirgin biçimde ucuz. Yoklama bir **ölçüm
  değil**, yapılabilirlik kontrolü; tuş başına bayt/gecikme iddiası R6.2'nin
  çift borcu olarak duruyor.
- **Dolgu opsiyonel kabul edildi.** Şart koşmak kanalı kodlayıcının bir uygulama
  ayrıntısına bağlardı; çözücü hem `YWJjZA==` hem `YWJjZA` okuyor.
- **`DockEvent::Update` ödünç veriyor, sahiplenmiyor.** İlk taslak çözülmüş
  hâli olayda taşıyordu; bu tuş başına üç `String` + bir `Vec` demekti. Ayna
  tarayıcının kendi tamponunda duruyor, `ShellLog` ve `Session::dock_state`
  `clone_from` ile alıyor — sabit durumda ayırma sıfır (`the_mirror_reuses_its_buffers`).
  Aynı sebeple `dock_state` kopya döndürmüyor, `&mut DockState` alıyor.
- **`DockStatus::Unavailable` planda yoktu, eklendi.** Phase yalnız "sınır
  aşımı görünür olsun" diyordu; aynı sinyal bozuk yükte de gerekiyor ve
  **metnin boşaltılması** onun ayrılmaz yarısı: gösteremediğimiz satırı bayat
  bırakmak, phase-4'te ızgara bastırılırken dock'un bir önceki komutu
  göstermesi olurdu. `Idle` ile `Unavailable` ayrı, çünkü phase-4'ün bastırma
  kararı ikisini ayırmak zorunda.
- **`region_highlight` ofsetleri sınırın bu tarafında normalize ediliyor.**
  zsh iki uzay kullanıyor (`P` öneki `PREDISPLAY`'e, öneksiz `BUFFER`'a bağlı);
  ikisini burada birleştirmek çizen tarafı `PREDISPLAY`'in uzunluğunu bilmekten
  kurtarıyor — R1.3'ün "çözülmüş geçer"i.
- **base64 çözücüde maske hatası yakalandı:** altı bitlik değeri maskesiz
  kaydırmak (`b << 4`) `u8`'i taşırıyor ve debug'da panik oluyordu —
  `bt-core`'da gerekçesiz panik yasak. Maskeler kodda gerekçesiyle duruyor.
- **`ShellPhase`'in doc'undaki bayat "Input Dock (014)" atfı 012'ye düzeltildi.**

### `/code-review` (riskli phase) — 5 bulgu, 4 giderildi

- **Giderildi:** modül doc'u `vte`'nin 1024 baytta kestiğini iddia ediyordu;
  `std` altında `osc_raw` sınırsız bir `Vec` ve kesme yok (`is_full` kolları
  `#[cfg(not(feature = "std"))]`). Tarayıcının bizim olmasının **gerçek**
  gerekçesi yazıldı: yük ulaşıyor ama okunmuyor.
- **Giderildi:** `region_highlight` ofsetleri metnin dışını gösterebiliyordu
  (bayat `BUFFER` anlık görüntüsü). Artık görüntü uzunluğuna kırpılıyor ve boş
  kalan kayıt düşüyor — kırpma borcu çizen tarafa devredilmiyor.
- **Giderildi:** `cursor` ile `Highlight::start` iki ayrı uzaydaydı ve
  `start`'ın doc'u ikisinin aynı uzayda olduğunu ima ediyordu. `cursor` da
  görüntü uzayına normalize edildi; aksi hâlde caret **her satırda** prompt
  boyu kadar kayardı.
- **Giderildi:** `parse_dock`'un bozuk kolu `line.status`'ü `Live` bırakıyordu;
  bugün gözlenemez ama tuzak. Durum artık orada da yazılıyor.
- **Waive — `vte`'nin `unhandled` maliyeti.** Ayrıştırıcı OSC 8133'ü tanımıyor
  ve yükü atmadan **önce** bayt başına bir `write!` ile tanı dizgisi kuruyor;
  dizgi `debug!`'tan önce kurulduğu için logger yokluğu kısa devre yapmıyor.
  Üç gerekçeyle bu sette düzeltilmiyor: (1) bizim kusurumuz değil,
  alacritty'nin tanımadığı **her** OSC'ye davranışı ve kalıcı çaresi yukarı
  akışta bir `log_enabled!` koruması; (2) maliyet **kare yolunda değil**
  okuyucu thread'inde, yani R1.3'ün "kare başına sıfır ayırma" iddiası
  duruyor; (3) aynı tuş vuruşunda kabuğun saf zsh base64 kodlaması zaten
  baskın terim, yani ölçülmemiş bir azınlık terimi için biçimi yeniden
  tasarlamak orantısız. Kaçış yolu (DCS taşıyıcısı) modül doc'unda adıyla
  kayıtlı; Karar 5'te **değerlendirilmemiş** bir alternatif, elenmiş değil.
  Kullanıcıya soruldu, OSC'de kalma seçildi.
- **`/code-review` `Skill` çağrısı arka planda forklandı** (skill'in kendi
  seçimi; `proje.md` ön plan istiyor). Sonucu gelmeden commit atılmadı.

## Checklist

- [x] İkinci OSC numarası seçildi ve gerekçesi doc'ta — `8133`, dışlama listesi
      grep'lenerek (`DOCK_OSC`)
- [x] Base64 çözme; bozuk gövde yoksayılıyor, panik yok — tablo + `chunks_exact`,
      dolgu opsiyonel
- [x] Dock kolunun kendi sınırı türetildi ve doc'ta gerekçeli — 64 KiB,
      aritmetiği `DOCK_PAYLOAD_LIMIT`'te
- [x] Sınır aşımı **görünür** bir sonuç (sessiz düşüş yok) —
      `DockStatus::Unavailable(Overflow)`, metin de boşaltılıyor
- [x] `DockState` yeniden kullanılan tampon; `Session::dock_state()` yaprak
      kilitten okuyor — elle `clone_from`, `&mut` out-param
- [x] Test: 133'ün davranışı değişmedi (mevcut tarayıcı sınamaları)
- [x] Test: sınır aşımı ve bozuk base64 — ayrıca ofset kırpma, bölünme,
      tampon yeniden kullanımı, iki kolun ayrılığı
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`; yarış iki profilde de
      yeşil, `race_dock_state_and_frame` eklendi)
- [x] Riskli phase: `/code-review` koştu — 5 bulgu, 4 giderildi, 1 waive
      (`vte`'nin `unhandled` maliyeti; gerekçe Uygulama Notları'nda,
      kullanıcıya soruldu)
- [x] Yayın etkisi yazıldı

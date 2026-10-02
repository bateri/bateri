# Phase 3 — Uzak örnek ve zamanlama makinesi (`bt-shell-common`)

## Özet

Yardımcı oturuma `bt_load` isteği, örneklerden `RemoteStats`'a giden saf
hesap ve örneklemenin ne zaman koştuğunu söyleyen saf durum makinesi; henüz
çağıran yok.

_Requirements: R4.1, R4.2, R4.3, R4.4, R7_

## Değişiklikler

- **`crates/bt-shell-common/src/remote_files.rs`** — `helper_script`'e
  `bt_load {seq} [p]` (`discussion.md` → Karar 2): `/proc/stat`'ın `cpu`
  satırı, `/proc/meminfo`'nun dört alanı (+ `MemFree`/`Buffers`/`Cached`
  yedeği), `/proc/loadavg`, `/proc/uptime`, `df -P /`'nin son satırı; `p`
  ile `/etc/os-release`'in `PRETTY_NAME`'i, `cpuN` satır sayısı ve
  `ps -eo comm,pcpu --sort=-pcpu`'nun ilk üçü (hata sessiz). `/proc/stat`
  okunamıyorsa `BT-NOPROC`. Cevap `BT-R`/`BT-END` çerçevesinde, satırları
  kendi etiketli (`BT-L …`); `parse_reply`'ın `bt_stat` sözleşmesi değişmez,
  `bt_load`'ın kendi ayrıştırıcısı var. Betik `sh -c`'ye sarılı ve fish giriş
  kabuğuna dayanıklı (`upload::remote_command`); süreç adı uzaktan geldiği
  için ayrıştırıcı kontrol karakterini atar, panik yok.
- **`crates/bt-shell-common/src/remote_helper.rs`** — `Query::Load { detail:
  bool }` → `Answer::Load(LoadReply)` (`LoadReply` ya ham sayaçlar ya da
  `NoProc`); `HelperSession`'a `bt_load`'u yollayan yöntem, zaman aşımı
  `LOAD_TIMEOUT` (tasarım sabiti). Açılış hatası ile açık oturum hatası
  `Load` için **ayırt edilebilir** olur, çünkü Karar 1'in iki kuralı farklı:
  `serve` bir `Query::Load`'un açılış hatasını `Err` değil
  `Ok(Answer::Load(LoadReply::Unreachable(metin)))` olarak döndürür, açık
  oturumdaki hata bugünkü gibi `Err(String)`. `Reply`'ın imzası
  (`Result<Answer, String>`) ve `Verify`/`Count`'un davranışı değişmez, yani
  `bt-shell-macos`'taki çağıranlar (`hyperlink.rs`, `uploader.rs`,
  `preview.rs`, `promise.rs`) dokunulmadan derlenir; yalnız `Answer`
  üstünde kapsamlı `match` yazan yer varsa yeni kola bir satır alır
  (bugün `hyperlink::verify_remote`'un kapanışı `Answer::Counted(_)`'ı adıyla
  eşliyor — `Answer::Load(_)` kolu aynı dosyada, aynı phase'de eklenir).
  Modül başlığındaki "closes when idle" cümlesi örneklemenin oturumu açık
  tuttuğunu söyler.
- **`crates/bt-shell-common/src/remote_stats.rs`** (yeni) —
  - `Sampler`: bir önceki CPU sayaçları ve geçmiş (en çok 8 seviye); `LoadReply`
    + `RemoteStatsMode` → `RemoteStats` (yüzdeler yuvarlanmış, seviye `⌊v/12.5⌋`
    7'ye kırpılı, geçmiş yalnız sparkline'da) ve popover'ın `Detail`'i (OS,
    çekirdek, CPU %, load, bellek/swap baytları, disk %, uptime, süreçler).
    Sayaç geri giderse (yeniden başlayan sunucu, taşma) fark atılır, o örnek
    CPU'suz.
  - `Schedule` (saf): girdiler — uzak nesil başladı/bitti, biçim (`Off`
    dahil), görünürlük, etkileşim (zaman damgasıyla), tik geldi (jetonla),
    cevap geldi (başarılı / `NoProc` / açılış hatası / oturum hatası), popover
    açık/kapalı. Çıktı — "şu jetonla şu kadar sonra tik kur", "istek yolla
    (`detail`)", "göstergeyi gizle", "dur". Karar 6'nın koşulları:
    `STATS_IDLE` (2 dk), ilk örnek hemen, CPU'suz ilk cevaptan sonra
    `FIRST_FOLLOW` (1 s), uçuşta tek istek, devamda geçmiş sıfırlanır,
    açılış hatası/`NoProc` nesli bitirir, oturum hatası bir kez yeniden dener.
    Saat dışarıdan (`Instant` argüman), sınamalar sahte zamanla.
  - Modül başlığı gerekçeye işaret eder (`.tasks/046-uzak-yuk-gostergesi/discussion.md`
    → Karar 1, 2, 6).
- **`crates/bt-shell-common/src/lib.rs`** — modül kaydı.
- **`crates/bt-shell-macos/src/hyperlink.rs`** (ve `Answer` üstünde kapsamlı
  `match` yazan öteki çağıranlar) — yalnız yeni `Answer::Load` kolunu
  "bağlantı yok" sayan tek satır; davranış değişmez.

## Kabul

- `remote_helper`'ın yerel `/bin/sh` sınaması emsaliyle `bt_load` uçtan uca:
  macOS'ta `BT-NOPROC` kolu, `make linux`'ta (Docker, gerçek `/proc`) sayılar
  makul aralıkta ve `p` ile çekirdek sayısı > 0. Sınama iki ortamı da
  adıyla kapsar (`cfg(target_os)` değil, `/proc/stat`'ın varlığı).
- Ayrıştırıcı: elle yazılmış cevaplar (eksik `MemAvailable`, `ps` yok, bozuk
  satır, kontrol karakterli süreç adı) panik değil beklenen sonuç.
- `Sampler`: iki örnekten CPU %, yuvarlama, seviye eşlemesi, 8'e kırpılan
  geçmiş, geri giden sayaç.
- `Schedule`: her durma ve devam koşulu, bayat jeton, uçuşta ikinci istek
  yok, açılış hatasından sonra tik yok, oturum hatasından sonra tek deneme.
- `make check` ve `make linux` yeşil.

## Checklist

- [x] `bt_load` betiği ve ayrıştırıcısı
- [x] `Query::Load` / `Answer::Load`, hata türünün ayrımı
- [x] `Sampler`
- [x] `Schedule`
- [x] Test: yerel `/bin/sh` uçtan uca (iki ortam)
- [x] Test: ayrıştırıcı, `Sampler`, `Schedule` senaryoları
- [x] Doğrulama geçti (`make check` + `make linux`)

## Uygulama Notları

- **`ps`'in sütun sırası ters:** betik `ps -eo pcpu,comm` istiyor (plan
  `comm,pcpu` diyordu) — `comm` boşluk taşıyabiliyor (`tmux: server`), sayı
  önde olunca ayrıştırma belirsizleşmiyor.
- **Zorunlu yalnız iki satır:** `cpu` satırı (en az dört sütun) ve
  `MemTotal`; ikisinden biri eksik ya da bozuksa `Malformed`. Load, uptime,
  disk, OS, çekirdek ve süreçler isteğe bağlı: bozuk değer o alanı boş
  bırakıyor, örneği düşürmüyor. Bilinmeyen `BT-L` etiketi atlanıyor (daha
  yeni betik), `BT-L` olmayan satır `Malformed`. `PRETTY_NAME`'in tırnakları
  Rust'ta soyuluyor; `df`'in son satırı `END`'de `$0`'a değil ana kuralda bir
  değişkene yakalanıyor (her awk `END`'de `$0`'ı tutmuyor).
- **`Eq` için tam sayılar:** `Answer` `Eq` türetiyor, yani load ortalamaları
  yüzde birlik, süreç `pcpu`'su onda birlik tam sayı (`LoadSample::load`,
  `Process::cpu`). Bellek baytla (kB × 1024, doymalı).
- **Tipler:** ham okumalar (`LoadSample`, `CpuCounters`, `Process`) ve
  ayrıştırıcı `remote_files`'ta (`parse_load`, `Ok(None)` = `BT-NOPROC`);
  `LoadReply` (`Sample`/`NoProc`/`Unreachable`) `remote_helper`'da.
  `HelperSession::ask`'ın yazma-okuma döngüsü `exchange`'e çıktı, `load` onu
  paylaşıyor — sıra numarası tek sayaç, yani geç bir `bt_load` cevabı bir
  `bt_stat`'ınki sanılamıyor (uçtan uca sınama bunu `bt_stat`'la bitiriyor).
- **Açılış hatasının iki yolu:** `serve`'de hem `HelperSession::open`'ın
  `Err`'i hem `RETRY_AFTER` içinde tutulan hata `Query::Load` için
  `Ok(Answer::Load(LoadReply::Unreachable))`. Sınama bunu çevirmenin tek ssh
  denemesiyle olduğunu da sayıyor.
- **`Sampler::take` biçimi `StatsForm` alıyor**, `RemoteStatsMode` değil:
  `Off`'ta istek hiç gitmiyor (`Schedule`), yani `Off` kolu ölü olurdu.
  Çıktı `Reading { stats, detail }`; geçmiş içeride her biçimde tutuluyor,
  dışarı yalnız `sparkline`'da çıkıyor (biçim değişince grafik boş başlamasın).
- **`Schedule`'ın yüzeyi:** `set_generation`, `set_form(on, interval)`,
  `set_visible`, `interaction`, `set_detail`, `tick(token)`,
  `answered(generation, Outcome)`; eylemler `Arm { token, after }`,
  `Request { detail, restart }`, `Hide`. `restart` sürücüye `Sampler::reset`
  dedirtiyor (devamda geçmiş sıfırlanır). "Dur" ayrı bir eylem değil: durmak
  tik kurmamak, etkin durdurmalar (nesil, `off`, görünürlük) jetonu da
  bayatlatıyor; edilgen durma (`STATS_IDLE`) tikte fark ediliyor.
  Planda tanımsız iki karar: **yeni nesil etkileşim sayılıyor** (kullanıcı
  az önce bağlandı; ⌘T'nin aynı host satırı tuşsuz doğuyor) ve **nesil
  bitince `Hide` yok** — değeri `Session` zaten `C`/`D`/`A`'da siliyor.
  Uçuşta istek varken istenen istek (devam, popover, yeni nesil) cevapta
  gidiyor; başka neslin cevabı yalnız worker'ı serbest bırakıyor, yeni nesli
  bitirmiyor. Başarılı bir cevap tek yeniden deneme hakkını geri veriyor.
- `/code-review` koşmadı: riskli phase tetikleyicisi yok (`.wgsl` yok, kilit
  dosyası değişmedi; mevcut worker'a bir sorgu türü eklendi, PTY okuyucu ya
  da render thread'iyle paylaşılan durum yok — `make test-race` gerekmedi).
- `make linux` ilk koşuda yeşil; `jobs::tests::the_process_table_reads_a_real_argv`
  bu sefer düşmedi.

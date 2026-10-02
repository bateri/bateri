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

- [ ] `bt_load` betiği ve ayrıştırıcısı
- [ ] `Query::Load` / `Answer::Load`, hata türünün ayrımı
- [ ] `Sampler`
- [ ] `Schedule`
- [ ] Test: yerel `/bin/sh` uçtan uca (iki ortam)
- [ ] Test: ayrıştırıcı, `Sampler`, `Schedule` senaryoları
- [ ] Doğrulama geçti (`make check` + `make linux`)

# Uzak dosyayı indirme ve önizleme

## Hedef

ssh/mosh oturumunda `ls` çıktısındaki bir dosyaya ⌘-tık onu geçici, salt
okunur bir kopyayla önizler; ⌘-sürükle Finder'a, sağ tık Downloads'a (ya da
seçilen yere) indirir. İndirmeler 037'nin kuyruğunda, ters yönde; ayarlarda
"Remote Files" kategorisi ve önizleme klasörünün kendiliğinden temizliği.
Gerekçeler `discussion.md` → Karar; ekranlar tasarım tuvalinde (`context.md`).

## Gereksinimler

- **R1** — Uzak bağlantı: uzak oturumda düz metin yol adayı bağlantı adayıdır
  ve **uzak** diye işaretlidir; `file://` uzakta kapalı kalır.
  - **R1.1** — Doğrulama yardımcı ssh oturumundan (Karar 10): var olan aday
    vurgulanır, cevap (dizin, ad) başına hatırlanır; doğrulanmamış aday
    vurgulanmaz.
  - **R1.2** — Göreli taban `remote_cwd`; boşsa göreli aday bağlantı değildir
    ve hedef etiketi "Remote folder unknown — enable OSC 7 on the server"
    der. Mutlak ve `~/` yollar çalışır.
  - **R1.3** — Yardımcı oturum tembel açılır, uzak oturumun nesli değişince ya
    da boşta kalınca kapanır; parola soramaz (`BatchMode`), açılamazsa
    bağlantı yoktur ve etiket nedeni söyler.
- **R2** — Aktarım iki yönlü: `ssh … tar c | tar x` indirme, upload'un
  ilerleme/iptal/disk dolu kurallarıyla; yerelde geçici ad + `rename`, iptal
  ve hata geçiciyi siler; inen öğe karantina etiketi taşır.
  - **R2.1** — Kuyruk yönden bağımsız: üç şerit — sıralı kuyruk (upload + sağ
    tık indirmesi), önizleme ve Finder'a bırakma (ikisi sıra beklemez).
  - **R2.2** — Metinler yönü okur: `↑`/`↓`, satır özeti `↑1 ↓2`, başlık öneki,
    sonuç satırı, durdurma sorusu, bildirim; "Show transfers (N)"; biten
    indirmede "Show in Finder", biten önizlemede "Open".
  - **R2.3** — Upload'un bugünkü davranışı ve sınamaları değişmez.
- **R3** — Sağ tık menüsü (uzak yol): dosyada Open Preview · Download to
  Downloads · Download To… · Copy Path · Copy as scp Path; klasörde Open
  Preview yok. scp yolu `-p` portunu taşır (`scp -P N`); argv'den
  çevrilemeyen seçenekler varsa yalnız `host:/yol`.
- **R4** — Onay sayfası yalnız gerektiğinde: klasör (dosya sayısı, boyut),
  hedefte ad çakışması (`download_conflict`: Ask → Keep both / Replace),
  yerelde yer yok (düğme kapalı + neden). Tek dosya sorusuz iner.
- **R5** — Önizleme (⌘-tık dosya):
  - **R5.1** — Kopya `{preview_dir}/{host}/{uzak mutlak yol}`'a iner, salt
    okunur (`0444`) olur ve açılır; klasöre ⌘-tık no-op.
  - **R5.2** — `preview_max_size`'ın üstünde önce sorar ("Open Preview",
    "Save to Downloads instead", metin geçici ve salt okunur olduğunu ve
    açılışta temizlendiğini söyler).
  - **R5.3** — Betik, program ve `x` bitli dosya varsayılan düz metin
    uygulamasıyla; bilinen içerik tipi kendi uygulamasıyla; geri kalanı düz
    metin.
  - **R5.4** — Uzaktaki boyut ve mtime yereldeki kopyayla aynıysa yeniden
    indirilmez.
- **R6** — Temizlik (Karar 9): açılışta saklama + boyut sınırı (en eski
  önce), günde bir kez yalnız saklama, Clear Now; çıkışta yok; bateri açıkken
  boyut yüzünden silme yok; bateri'nin yazdığından farklılaşmış kopya
  silinmez, Downloads'a taşınır ve bildirilir.
- **R7** — ⌘-sürükle: bağlantı basışında eşiği aşan hareket file promise
  sürüklemesi başlatır (dosya ve klasör), eşik altı bırakma önizler;
  Finder'ın verdiği hedefe indirir, sıra beklemez, ilerlemeyi `NSProgress`
  ile yayınlar.
- **R8** — Ayarlar: `[remote]` yeni anahtarlar (`preview_max_size`,
  `preview_read_only`, `preview_dir`, `preview_keep`, `preview_limit`,
  `download_dir`, `download_conflict`, `download_notify`), varsayılan,
  tanı, şablon, `docs/AYARLAR.md`, round-trip; ayar penceresinde "Remote
  Files" kategorisi (popup/switch satırları, klasör seçici, kullanım +
  Clear Now).

## Yaklaşım

1. Saf zemin: `bt-core`'da uzak hit'in kapısı ve işareti, `[remote]` ayar
   anahtarları ve "Show transfers" metni; `bt-shell-common`'da yardımcı
   oturumun protokolü (betik + cevap ayrıştırma), indirme betiği, scp yolu,
   uzak açma politikası, önbellek yol eşlemesi ve temizlik planlayıcısı —
   hepsi I/O'suz ve sınanır.
2. Kuyruk genelleşir (`Uploads` → `Transfers`, yön + şerit) ve indirme süreci
   (`download::transfer`) eklenir; upload'un yolu aynen geçer.
3. Uzak doğrulama ve menü: `hyperlink.rs` uzak hit'i yardımcı oturuma sorar,
   sağ tık menüsü indirir, onay sayfası ve karantina.
4. Önizleme ve temizlik: ⌘-tık, sınır sorusu, açma, önbellek, açılış/günlük
   süpürme, Clear Now, farklılaşmış kopyanın taşınması.
5. ⌘-sürükle: jest eşiği, file promise kaynağı, Finder şeridi.
6. Ayar penceresinin kategorisi.

## Kapsam Dışı

- Önizlemeyi düzenleyip sunucuya geri yükleme (ayrı set).
- Yerel bağlantıların sürüklenmesi (yerel dosya zaten Finder'da).
- Uzak kabuğun dizinini OSC 7'siz bulmak (Karar 2-C reddedildi).
- Uzakta `file://` bağlantıları (`ls --hyperlink`).
- Linux kabuğunun karşılığı (`bt-shell-linux` henüz yok); saf yarı
  `bt-shell-common`'da olduğu için `make linux` onu derler.

## Akış

```
⌘-hover (uzak)
  link_at ─► hit{uzak, adaylar} ─► yardımcı oturum: stat(adaylar, remote_cwd)
           ─► ilk var olan ─► vurgu (+ önbellek: dizin/ad → tür, boyut, mtime)

⌘-tık dosya ──► boyut ≤ sınır? ──hayır─► sor (Open Preview / Save to Downloads)
                     │evet
                     ▼
     önbellekte aynı boyut+mtime? ─evet─► aç
                     │hayır
                     ▼
     Transfers[önizleme şeridi]: ssh tar c │ tar x → geçici → rename → 0444 → aç

sağ tık › Download ─► (klasör/çakışma/yer yok ise sayfa) ─► Transfers[kuyruk]
⌘-sürükle ─► NSFilePromiseProvider ─► Finder hedefi ─► Transfers[Finder şeridi]

açılış ─► temizlik(saklama + boyut) ; günlük ─► temizlik(saklama) ; Clear Now
  farklılaşmış kopya ─► Downloads'a taşı + bildirim
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| phase-6 | |
| kapı | |

# Odak sorgusu — Tartışma

## Karar 1: Push mu pull mu → ✅ yalnız pull

Gizleme kararı tek seferlik: oturum bittiği ya da sorduğu anda kullanıcı
pane'e bakıyorsa durumu görmüştür, sonradan sekmeden çıkınca yeniden
göstermeye gerek yok. Pull tek başına yetiyor ve dışarıya "sekme değiştirdi"
zamanlaması bile sızmıyor. Distributed notification reddedildi.

## Karar 2: Ne söyleniyor → ✅ yalnız sorulan pane, iki jeton

`bateri focus [--pid P] bateri://tab/<UUID>` tek jeton satırı basar:

- `pane=live focused=1 idle=4` — pane açık; `focused` bateri etkin, pencere
  key ve bu pane pencerenin odaktaki pane'i; `idle` bu pane'e son girdiden
  beri geçen **tam saniye** (aşağı yuvarlanmış).
- `pane=none` — bu UUID'yi tanıyan açık bir pane yok (kapanmış pane, çalışan
  bateri yok).
- `pane=unknown` — cevap alınamadı (zaman aşımı, tanınmayan tel; Karar 8).

Yetki UUID'nin kendisi: soran onu bilmek zorunda, başka pane, başlık, dizin ya
da sekme listesi hiçbir yoldan çıkmıyor. Jeton satırı makine sözleşmesi
(`CLAUDE.md` → Dil): jeton silinmez, eklenir.

## Karar 3: Bölmede görünür ama odakta olmayan pane → ✅ `focused=0`

Yalnız odaktaki pane "bakılıyor" sayılır; soluk pane'e bakmak ile ona
çalışmak ayrı şeyler ve yanlışın yönü güvenli (evlat fazladan gösterir,
eksik göstermez).

## Karar 4: `idle` neyi sayıyor → ✅ pane'in kendi girdisi, saniye çözünürlüğü

Sayılan: o pane'in view'ına giden tuş, fare basışı, tekerlek, fare hareketi
ve pane'in penceresinin key olması — bugünkü `note_interaction` noktalarının
ta kendisi, yani ikinci bir girdi tanımı doğmuyor. Hiç girdi almamış pane'in
damgası doğumu. **Saniyeye yuvarlama bir güvenlik kararı:** tuş anlarının
milisaniyelik dizisi yazılan metni (parolayı) tahmin etmeye yarayan bilinen
bir yan kanal; "baktı mı" sorusu saniyeyle cevaplanır. Sistem genelinin boşta
süresi bateri'nin konusu değil, evlat onu kendisi okuyor; eşikler ve kural
evlat'ın.

**Bilinen sınır:** arama alanına yazmak `idle`'ı sıfırlamıyor (tuşlar view'a
değil alana gidiyor); `focused` yine 1.

## Karar 5: Kanal → ✅ örnek dizininde uzun yaşayan bir unix soketi (teknik karar)

Her bateri örneği kendi örnek dizininde (`Masters`'ın dizini, `bases()`'in
**ilki**) bir `focus` soketi dinler. Dinleyici `Masters`'ın açılıştaki
süpürme thread'inde kuruluyor: dizin zaten orada doğuyor (ikinci bir
`prepare_instance` yolu yok) ve süreli koşuda `masters` hiç kurulmadığı için
hermetik kapı bedavaya geliyor. Gerekçe: dizin 0700, sahibi doğrulanıyor —
yeni bir güven sınırı icat edilmiyor.

**Bulma:** `bateri focus [--pid P] <url>`. evlat çalışan bateri'nin pid'ini
zaten biliyor (`SessionHost.App.pid`); `--pid` verilince köklerde sahibi
`P` olan **tek** dizine sorulur, verilmezse sahibi canlı bütün örneklere
(sırayla, `pane=live` diyen ilki kazanır). Canlı örnekleri sayan mantık
`Masters::sweep`'in içindekiyle **paylaşılır** (tek bir `live_instances`,
süpürme ölüleri, sorgu canlıları alır); aynı pid'in iki kökteki dizini tek
örnek sayılır.

**Süpürme:** soketin adı `remove_instance`'ın kendi adlar kümesine **açık
bir sabit** olarak girer (`our_socket_name`'e değil — onu `sweep_flat` da
kullanıyor); yoksa `remove_dir` boş olmayan dizinde düşer ve dizin kalıcı
olarak birikir. ⌘Q'daki `close_all` aynı kümeyle dizini kaldırıyor.

- **Reddedilen — `bateri://` ile cevap:** URL geri değer döndüremez ve 038'in
  "URL yalnız odaklar" değişmezini deler.
- **Reddedilen — Apple Events / `NSXPCConnection`:** biri evlat'a izin
  sorduruyor, öbürü Mach servis kaydı ve paket düzeni istiyor.

## Karar 6: Cevabı kim hesaplıyor → ✅ sorgu anında ana thread (teknik karar, panelden sonra)

Soket thread'i isteği okuyup cevabı **ana kuyruğa** sorar ve kısa bir
sınırla bekler; ana thread cevabı canlı durumdan hesaplar:
`NSApp.isActive && pencere key && pencerenin focused_pane()'i == sorulan
pane` ve `idle` pane'in son girdi damgasından. Yeni bir paylaşılan durum,
ayna tablo ya da olaya bağlı yazım noktası **yok**: bayat bir `focused=1`
(güvensiz yön — evlat her uyarıyı gizlerdi) üretecek yer kalmıyor. Bedel
sorgu başına bir ana kuyruk turu; evlat yalnız olay anında soruyor. Render
yolu etkilenmiyor: bekleyen soket thread'i, ana thread yalnız mikro
saniyelik bir closure koşuyor.

**Damga:** pane'in ivar'ı (ana thread'e ait, kilitsiz), tek yazarı
`TerminalPane::note_interaction`; hiç girdi almamış pane'de doğum anı.
Saati **uykuyu sayan** bir monoton saat (macOS'ta `CLOCK_MONOTONIC`,
Linux'ta `CLOCK_BOOTTIME`): Rust'ın `Instant`'ı macOS'ta uykuda durur ve
kapak iki saat kapalı kaldıktan sonra `idle=4` derdi — evlat "bakıyor" deyip
gizlerdi.

**Bilinen sınır:** ekran kilidi `windowDidResignKey:` üretmiyor, `focused=1`
kalıyor; `idle` büyüdüğü için kararı evlat'ın eşiği veriyor. Pencerenin key
oluşu bütün pane'lerinin damgasını tazeliyor; odakta olmayan pane'de
`focused=0` olduğu için zararsız.

## Karar 7: Uzak oturum → ✅ ek kod yok

ssh içindeki Claude için evlat UUID'yi `LC_BATERI_TAB_URL`'den alır (049) ve
soruyu **bu Mac'teki** bateri'ye sorar; pane yereldir. bateri tarafında
değişen bir şey yok.

## Karar 8: Takılma, zaman aşımı ve sürüm → ✅ üç sınır, ayrı bir "bilinmiyor" (teknik karar, panelden sonra)

- **İstek** `focus 1 <UUID>\n` (`1` tel sürümü); **cevabı örnek kendisi
  üretir**, CLI olduğu gibi aktarır — yeni jeton CLI değişmeden eklenir,
  eski örnek daha az jeton verir.
- **Sınırlar:** sunucuda bağlantı başına okuma ve ana kuyruğu bekleme
  sınırı (bir istemci accept döngüsünü kilitlemesin); CLI'de örnek başına
  connect/read sınırı ve toplam bir son tarih. Sayılar tasarım sabiti,
  yüzlerce milisaniye mertebesinde; askpass'in 30 dakikalık `ANSWER_WAIT`'i
  emsal **değil**.
- **Bilinmiyor ≠ yok:** zaman aşımı, tanınmayan tel sürümü ya da
  bozuk cevap `pane=unknown` ve ayrı bir çıkış kodu; `pane=none` yalnız
  "hiçbir canlı örnek bu UUID'yi tanımıyor" demek. evlat ikisini de
  "göster" okuyabilir, ama ayrım sınanabilir kalsın.

## Karar 9: Tanınmayan alt komut → ✅ GUI açmadan çık (teknik karar, panelden sonra)

Bugün tanınmayan her argv GUI açılışına düşüyor: `focus`'u bilmeyen bir
bateri'yi çağıran evlat her bildirimde ikinci bir pencere doğururdu. Bundan
sonra `main.rs` `-` ile başlamayan, tanınmayan bir argv[1]'de GUI açmadan
sıfırdan farklı kodla ve tanı satırıyla çıkar (LaunchServices'in `-psn_…`
argümanı `-` ile başlıyor, etkilenmez). Bugün kurulu eski sürüm için çare
evlat'ta: çalışan kopyanın sürümünü kapılamak (`context.md`).

## Muhakeme (2026-10-03)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU (hafif) |
| Codebase-fit | SORUNLU (hafif) |
| İşletme | SORUNLU |

**Kabul edilen itirazlar → plan değişikliği:**
- Pano + atomik + altı yazım noktası gereksiz; cevap sorgu anında ana
  thread'de hesaplanabilir (Sadelik) → Karar 6 yeniden yazıldı. Bu, İşletme'nin
  "bayat `focused=1` güvensiz yön" ve "resign'da panoyu silme yarışı"
  itirazlarını ve Codebase-fit'in "odak yazımı tek noktaya insin" itirazını
  da konusuz bırakıyor.
- Örnek dizini zaten açılışta doğuyor, sahibi `Masters` (Codebase-fit) →
  dinleyici `Masters`'ın süpürme thread'inde, ikinci `prepare_instance` yok,
  hermetik kapı `masters == None`'dan (Karar 5); `context.md` düzeltildi.
- `ssh-argv` örneği adıyla buluyor, "aynı bulma yolu" yanlıştı (Codebase-fit)
  → canlı örnek sayımı `sweep` ile paylaşılan tek fonksiyon (Karar 5).
- evlat pid'i biliyor (İşletme) → `--pid` birincil yol, hepsini dolaşmak
  yedek (Karar 5).
- Soket adı `remove_instance`'ın kümesine açık sabit olarak, `our_socket_name`'e
  değil (Codebase-fit) → Karar 5.
- `Instant` uykuda duruyor (İşletme) → uykuyu sayan saat (Karar 6).
- Zaman aşımı sözleşmesi yok, askpass'inki yanlış emsal (İşletme) → Karar 8.
- Eski binary tanımadığı alt komutta GUI açıyor (İşletme) → Karar 9 +
  evlat'ın sürüm kapısı (`context.md`).
- `fits()` boşluk/`%` reddi ssh'ın kaygısı, odak soketininki değil
  (İşletme) → yalnız `SUN_PATH` uzunluğu sınanır (phase-1).

**Reddedilenler:**
- Tek phase (Sadelik) — sadeleşmeden sonra da iki ayrı doğrulanabilir yarı
  var: `bt-shell-common`'daki tel, sunucu, istemci ve süpürme saf ve sahte
  bir cevaplayıcıyla birim sınanıyor (`make linux` dahil); AppKit bağlaması,
  alt komut ve `main.rs` kuralı ayrı bir commit ve `make bundle` istiyor.
- `idle`'ı yük göstergesinin `Schedule::interaction`'ından okumak (Sadelik)
  — damga zaten orada, ama alanın anlamı zamanlayıcının ("örnekleme sürsün
  mü") ve saati `Instant`; ayrı bir ivar aynı hunide bir satır.

## Karar (2026-10-03, kullanıcı onayı + teknik karar)

- **Seçilen:** pull-only `bateri focus [--pid P] <url>` →
  `pane=live focused=… idle=…` / `pane=none` / `pane=unknown`; örnek
  dizinindeki unix soketinden, cevap sorgu anında ana thread'de (Karar 1–9).
  Karar 1, 2, 4 kullanıcıyla konuşuldu; 3 öneri olarak sunuldu; 5–9 teknik,
  6–9 panelden sonra.
- **Reddedilen:** push / distributed notification (Karar 1); genel durum
  API'si — sekme listesi, başlıklar; ms çözünürlüklü `idle` (Karar 4);
  `bateri://` cevabı, Apple Events, XPC (Karar 5); ana thread'in yazdığı
  pano (Muhakeme).

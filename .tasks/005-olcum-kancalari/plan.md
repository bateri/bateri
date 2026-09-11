# Ölçüm kancaları

## Hedef

`/measure`'ın bugün "ölçüm aracı yok" dediği yerde sayı üretmek: kare
süresinin CPU ve GPU tarafı, açılış süresi ve atlas doluluğu. Sayıyı üreten
yol, ölçmediği zaman **hiç var olmamış gibi** davranmalı.

## Gereksinimler

- **R1** — Ölçüm için sabit bir yük profili, `smoke_shell`'den **ayrı**.
  - **R1.1** — `smoke_shell` dokunulmaz; `hucre=8 glif=6 kural=15` ve üç
    sınamanın sahipliği onda kalır.
  - **R1.2** — `run_seconds.is_some()`'ın bugün taşıdığı üç anlam (sabit
    shell, deadline, bekçi) ayrılır; yük seçimi bağımsız olur.
- **R2** — Atlas doluluğu `bt-shell`'e ulaşır, katman yönü bozulmadan.
  - **R2.1** — `bt-gpu` yeniden yayımlar (`Renderer::cell_metrics` deseni);
    `bt-shell`'e `bt-atlas` kenarı **eklenmez**.
  - **R2.2** — Duman satırı `yuva=U/T` jetonu kazanır.
- **R3** — Zaman iki kaynaktan okunur; üçüncüsü (düşen kare) **ölçülmez**.
  - **R3.1** — CPU iki ayrı aralık: `session.frame()` ve `draw`. Tek aralık
    002 #1'i (kilit beklemesi) ayıramaz.
  - **R3.2** — GPU deltası mevcut tamamlanma bloğundan okunur. Blok kare
    başına **kurulmaz** (`renderer.rs:318-323` bunu yasaklıyor), dolayısıyla
    damga closure ile yakalanamaz: paylaşılan slot gerekir.
  - **R3.3** — Açılış damgası `main.rs`'te, `Renderer::system_default()`
    çağrısından (`lib.rs:43`) **önce** alınır.
- **R4** — Enstrümantasyon kapılıdır ve kapı kapalıyken yoktur.
  - **R4.1** — Kapı kapalıyken tek bir `Instant::now()` bile çağrılmaz.
  - **R4.2** — env yalnız `main.rs`'te okunur, `Options`'a **tipli** alan
    olarak girer; kodun derinine `env::var` saçılmaz.
  - **R4.3** — Kare başına I/O ya da kilit yok; örnekler önceden ayrılmış
    bellek içi halkada birikir.
- **R5** — Rapor mevcut jeton satırını genişletir; **dosya yazılmaz**.
  - **R5.1** — p95 ve en kötü değer süreç içinde hesaplanır.
  - **R5.2** — `ornek=` örnek sayısını taşır: örnekleme sessizce durursa
    (örtülen pencere) az örnek üstünden hesaplanan p95 *iyi* görünür.
  - **R5.3** — `profil=` debug mü release mi söyler: `make duman` debug
    koşuyor, `/measure` release şart koşuyor.
  - **R5.4** — Jeton sözleşmesi: **silinmez, eklenir**.
  - **R5.5** — Çıktı `report_and_exit` içinde **açıkça** yazılır; `Drop`'a
    güvenen hiçbir yol kullanılmaz (`process::exit` `Drop` koşturmaz, bekçinin
    `_exit(70)`'i atexit'i bile atlar).
  - **R5.6** — **Örnek tabanı.** Az örnek üstünden hesaplanan p95 basılmaz.
    `ornek=` bir süs değil **kapı**: taban altında sayı **hesaplanmaz**,
    jeton "yetersiz" der ve sebebini söyler.

    > **Bu gereksinimin ilk gerekçesi yanlıştı ve düzeltildi.** Orkestratör
    > phase-2 sonrası `BT_SCROLL_TEST=1 BT_RUN_SECONDS=5` → `kare=3` ölçtü ve
    > bunu "pencere örtülü, sistem `CAMetalDisplayLink`'i askıya alıyor" diye
    > açıkladı. **Örtülülük gerçek** (`occlusionState` `Visible` taşımıyor,
    > phase-1'de okundu) ama **sebep o değil**: phase-2b'nin düzeltmesinden
    > sonra aynı pencere durumunda `kare=594` (5 sn) ve `kare=230–234` (2 sn)
    > ölçüldü. Gerçek sebep kapanış kilitlenmesinin kök nedeniydi — master'ı
    > kimse boşaltmayınca kuyruk doluyor, okuyucu tıkanıyor, kirli satır
    > düşmüyor. Yani ölçülen şey tavan değil **bozuk bir koşuydu**;
    > korelasyon nedensellik sanıldı.
    >
    > Gereksinim ayakta kalıyor çünkü dayanağı bu ölçüm değil ilkedir: az
    > örnek üstünden hesaplanan p95, sebebi ne olursa olsun anlamsızdır.

- **R6** — Boşta sıfır kare korunur ve bunun bir bekçisi vardır.
  - **R6.1** — `make duman` kapısı `kare` için **üst sınır** kazanır; bugünkü
    `n > 0` kancaların ürettiği boşta kareyi göremez.
- **R7** — Belgeler kodla uyumlanır; borç cümleleri **silinmez, yeniden
  yazılır**.
  - **R7.1** — `CLAUDE.md`'nin "`cargo bench` satırı geri gelir" sözü "bench
    seti bekliyor"a çevrilir: bu set bir borcu başka bir borçla takas ediyor.
  - **R7.2** — Kanca adları kodla uyumlanır. `BT_FRAME_LOG` adı **yalan**
    olurdu (dosya yok) → `BT_FRAME_STATS`; `BT_STARTUP_TRACE` ayrı bayrak
    değil, aynı bayrağın altında tek sayı.
  - **R7.3** — `docs/OLCUMLER.md` bu sette **yazılmaz**; ilk `/measure` kurar.

- **R8** — Kapanış **sınırlı sürede** döner; ölçüm koşusu asılmaz.
  - **R8.1** — Mekanizma **teşhis edilir**, tahminle düzeltilmez.
    `shutdown()`'ın bugünkü doc'u asılmayı `trap '' HUP` ile açıklıyor, oysa
    üretici (`load_shell`) HUP'ı yutmuyor: doc kodu kapsamıyor.
  - **R8.2** — Sınır bir `const` ve gerekçesi yorumda. Bekçiden (`bt-shell`,
    yalnız `BT_RUN_SECONDS` yolu) **ayrı**: bu `bt-core`'da ve her yolda
    çalışır, yani etkileşimli kullanımdaki "kesen yok" hâlini de kapatır.
  - **R8.3** — Kanıt ölçülür: `BT_SCROLL_TEST` en az 8 koşuda **0 asılma**
    (öncesi 5/8).

## Yaklaşım

1. **Phase-1 `bt-core` + `bt-gpu` + `bt-shell`** — ölçüm olmayan boru: ayrı
   yük profili (`load_shell`), `Workload` ile üç anlamın ayrılması, doluluğun
   `bt-gpu` üzerinden yeniden yayımı, `yuva=` jetonu ve duman kapısının üst
   sınırı. **Bekçi enstrümantasyondan önce kuruluyor** — koruduğu şey gelince
   zaten silahlı olsun diye.
2. **Phase-2 `bt-shell` + `bt-gpu`** — zaman yakalama: `Options`'a tipli
   alanlar, env kenarda bir kez, CPU'nun iki aralığı, GPU'nun paylaşılan
   slotu, açılış damgası, örnek halkası. Henüz hiçbir şey basılmaz; kapı
   kapalıyken hiçbir yol değişmez.
2b. **Phase-2b `bt-core`** — kapanışın sınırlı beklemesi. Phase-2 sonrası
   ölçüldü: ölçüm yükünün kapanışı 8 koşunun 5'inde asılıyor (`exit 70`, jeton
   satırı hiç çıkmıyor), smoke yükü 4/4 temiz. Setin bütün ürünü ölçüm koşusu
   olduğu için bu bir kusur değil **ürünün güvenilmezliği**. Rapordan **önce**
   geliyor: phase-3 `## Yöntem`'i yazacak ve asılma kapanmazsa o belge "asılan
   koşu atılır" demek zorunda kalır.
3. **Phase-3 `bt-shell` + belgeler** — rapor: p95 ve en kötü değerin süreç
   içinde hesabı, jetonların basılması, `CLAUDE.md` / `/measure` skill'i /
   `context.md` şablonunun kanca adlarıyla uyumlanması.

## Kapsam Dışı

Giriş gecikmesi zinciri (`BT_INPUT_LATENCY_SAMPLES`) — bekleyen on iki
iddianın hiçbiri gecikme iddiası değil, üstelik zincirin orta halkaları bu
depoda değil (`alacritty_terminal::EventLoop`). Gecikme iddiası doğuran ilk
sette gelir.

`cargo bench` hedefleri ve `criterion` — yeni bağımlılık, yani ayrı bir
mimari karar. **Bedeli açıkça kabul ediliyor:** 003 #1 ve 003 #2'nin bench
yarısı bu setten sonra da açık kalır ve `/measure 003` onlara "ölçüm aracı
yok" demeye devam eder.

Logger (`tracing`) — aynı enstrümantasyon damarından geçiyor ama ayrı bir
bağımlılık kararı; seti "ölçüm + gözlemlenebilirlik" diye şişirir.

Düşen kare sayımı, viewport kaydırma yükü (viewport kaydırmanın kendisi yok),
`docs/OLCUMLER.md`'nin yazılması, bellek ve sekme ölçümleri.

## Göç

Kullanıcının makinesinde değişen bir şey yok: yeni ayar anahtarı, tema biçimi,
`TERM`/terminfo ya da shell entegrasyon dosyası doğmuyor. `make duman` çıktısı
jeton **ekler** (`yuva=`, ardından ölçüm jetonları), eskisini korur.

## Akış

```
main.rs   BT_FRAME_STATS / BT_SCROLL_TEST okunur (TEK YER)
          Instant::now()  ←── Renderer::system_default()'tan ÖNCE (R3.3)
             │
             ▼  Options { run_seconds, workload, stats: Option<Stats> }
        bt-shell::run  ──────────────────────────────► AppDelegate ivar
             │
             ▼
   needs_update (link.rs:257)
        ├─ t0 ──► session.frame(sink) ──► t1      CPU aralık 1 (kilit + parse + grid)
        ├─ t1 ──► renderer.draw ───────► t2       CPU aralık 2 (encode)
        │              └─ completion bloğu (renderer.rs:321)
        │                    └─ GPUStartTime/EndTime ──► paylaşılan slot
        ▼                       (blok kare başına KURULMAZ — R3.2)
   örnek halkası (önceden ayrılmış, kare başına I/O yok — R4.3)
             │
             ▼
   report_and_exit (app.rs:415) ── p95 + en kötü hesaplanır, jetonlar basılır
        kare=N hucre=K glif=G kural=R yuva=U/T ornek=S profil=debug
        cpu_kare=… cpu_encode=… gpu=… acilis=… pipeline=ok
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-1 | ✅ | `9788d95` |
| phase-2 | ✅ | `8df1ef6` |
| phase-2b | ✅ | `e991d78` |
| phase-3 | | |

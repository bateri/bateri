# Ölçüm kancaları — Bağlam

## Mevcut Durum

Kare yolu bugün şöyle: `Waker::wake` (`link.rs:76-102`) kare talebi doğurur,
display link `needs_update`'e girer (`link.rs:257`), `session.frame(sink)`
çağrılır (`link.rs:276`), `renderer.draw` encode eder (`renderer.rs:390`),
`presentDrawable` + `commit` ile kapanır (`renderer.rs:379-380`).

**Tamamlanma bloğu zaten var** (`addCompletedHandler`, `renderer.rs:370`;
blok `Renderer::completion`, `renderer.rs:321-341`) ama yalnız `status()` ve
`error()` okuyup `frames` sayacını artırıyor. `GPUStartTime`/`GPUEndTime`
**hiçbir yerde okunmuyor**; display link'in kendi zaman bilgisi (target ve
actual timestamp) da okunmuyor — `update` yalnız `drawable()` için
kullanılıyor (`link.rs:289`).

Sayaçlar koşulsuz ve ucuz: `frames` tek bir `Relaxed` `fetch_add`
(`renderer.rs:337`), üç `last_*` sayacı `draw` içinde commit'ten önce
yazılıyor (`renderer.rs:373-378`) ve yalnız `report_and_exit`'te okunuyor
(`app.rs:416-419`).

**Üretim kodunda hiç zamanlama yok.** `std::time::Duration` var (bekçi
uykusu), `Instant` yalnız `#[cfg(test)]` altında (`session.rs:819`).
`SystemTime`, `mach_absolute_time`, `CACurrentMediaTime`: yok. `[[bench]]`,
`criterion`, `benches/`: yok.

Env kancasının bugünkü **tek örneği** `BT_RUN_SECONDS` ve örüntüsü net: bir
kez kenarda okunuyor (`main.rs:15-24`), tipli bir `Option` olarak
`Options.run_seconds` → `bt_shell::run` → ivar diye taşınıyor
(`lib.rs:23-26`, `lib.rs:48`, `app.rs:132`). Kodun derininde `env::var`
çağrısı yok. Ayrı bir "duman modu" bayrağı da yok — tek koşul
`run_seconds.is_some()`.

## Motivasyon

`/measure` bugün sayı değil **"ölçüm aracı yok"** döndürüyor ve bu üç seti
birden kilitliyor: 002, 003 ve 004'ün üçü de yalnız bu yüzden 🔨 duruyor.

Sözleşme yazılı, kod yok. `BT_FRAME_LOG`, `BT_SCROLL_TEST`,
`BT_STARTUP_TRACE` ve `BT_INPUT_LATENCY_SAMPLES` adları **dört belgede**
geçiyor — `CLAUDE.md`, `.claude/is-akisi/proje.md`, `/measure` skill'i ve
hatta `context.md` şablonu — ve `crates/` altında **sıfır** kez. Aynı şekilde
`docs/OLCUMLER.md` yok; `/measure` skill'i o dosyanın `## Yöntem` ve
`## Nasıl yeniden ölçülür` bölümlerini okumakla **başlıyor**.

`CLAUDE.md` bunu kendi borcu sayıyor ve kapanışını da tarif ediyor:
*"kancalar gelince bu cümle kalkar ve `cargo bench` satırı yukarıdaki bloğa
geri gelir."*

Sıranın gerekçesi `docs/YOL-HARITASI.md`'de: taban, **bir sonraki büyük render
değişikliğinden önce** alınırsa "hangi set yavaşlattı" sorusu cevaplanabilir
olur; sonra alınırsa o soru kalıcı olarak cevapsız kalır.

Referans kanca envanteri ve gecikme zincirinin halkaları
`docs/ARASTIRMA.md` → "Shell entegrasyonu" bölümündedir.

## Kanıt

**Bekleyen on iki iddianın dağılımı** (`.tasks/00{2,3,4}-*/teslim.md`):

| Tür | Sayı | Hangileri |
|---|---|---|
| Kare süresi | 8 | 002 #1–2, 003 #1, #2, #5, 004 #3, #4, #5 |
| Açılış / ölçek değişimi | 2 | 003 #4, 004 #1 |
| Atlas doluluğu | 2 | 003 #3, 004 #2 — zamanlama değil, **sayaç** (`occupancy()` zaten var) |
| **Giriş gecikmesi** | **0** | — |

İddialar bağımsız da değil: 004 #4 kendi metninde *"003 B.1 #2 ve #5'in"*
aynısı olduğunu söylüyor. Yani tek bir kare süresi koşusu birden çok iddiayı
kapatıyor.

**Sözleşmenin denetimsizliği kendi kaydını bozmuş.** 002'nin `teslim.md`'si
"bu set hiçbir kare süresi/gecikme iddiası taşımıyor" diyordu; aynı dosya
birkaç satır aşağıda iki kare süresi iddiasını sayıyor. `/measure 002` tam bu
dosyayı okuyor ve cümleyi "iddia yok" diye okuyan ölçümü atlardı. `4f00803`
ile düzeltildi — kanca seti açılmadan önce bulunmasının tek sebebi bu setin
kanıt toplaması.

**Gecikme zincirinin orta halkaları bu depoda değil.** `Session::write`
(`session.rs:684-689`) yalnız `Msg::Input` kanalına gönderiyor; gerçek fd
`write()` ve PTY okuyucu thread `alacritty_terminal::EventLoop` içinde
yaşıyor (`session.rs:430-446`). Yani *"PTY yazıldı → echo okundu"*
halkalarını ölçmek, bilerek kapsüllediğimiz bir bağımlılığın içine girmeyi
gerektirir.

## Mevcut Mimari

```
       Waker::wake (link.rs:76)
            │  dirty.mark() HER ZAMAN (link.rs:79)
            │  link yalnız Gate.is_open() iken açılır (link.rs:80-101)
            ▼
    needs_update (link.rs:257)
            │
            ├─ frame.clear (link.rs:269)
            ├─ session.frame(sink) (link.rs:276)  ── dirty.swap(false) (session.rs:467)
            │        └─ None dönerse → link.setPaused(true) (link.rs:279-281)
            │           ◄── BOŞTA SIFIR KARENİN TEK KESME NOKTASI
            ├─ renderer.draw (link.rs:289 → renderer.rs:390)
            │        ├─ encode_bg (renderer.rs:462)
            │        ├─ encode_glyphs (renderer.rs:505)
            │        ├─ last_* sayaçları (renderer.rs:373-378)
            │        ├─ addCompletedHandler (renderer.rs:370)
            │        │     └─ Renderer::completion (renderer.rs:321-341)
            │        │        status()/error() okur, frames++ (renderer.rs:337)
            │        │        ◄── GPU ZAMANI İÇİN HAZIR KANCA NOKTASI
            │        ├─ presentDrawable (renderer.rs:379)
            │        └─ commit (renderer.rs:380)
            ▼

  Okuyucu thread (alacritty EventLoop — BU DEPODA DEĞİL)
            │
            └─ Adapter::send_event → Event::Wakeup
                   dirty.store(true) (session.rs:350)
                   wake.wake() (session.rs:351)
                   ◄── Term kilidi tutulurken koşabilir (wake.rs:5-7)
```

**Boşta sıfır kareyi bozmanın üç yolu** (`link.rs`'ten okundu, plan bunlardan
kaçınmak zorunda):

1. Periyodik/timer'dan `wake()` ya da `request_frame` çağırmak
   (`link.rs:79`, `link.rs:387`).
2. Tamamlanma bloğunun `Ok` kolundan yeniden kare istemek — `Retry` yalnız
   `Err`'de uyandırıyor (`link.rs:183-188`) ve `FailureStreak` art arda
   ikinci hatada susuyor (`link.rs:209-211`).
3. `Session::frame`'i ikinci bir tüketicinin çağırması: `dirty` bayrağını
   çalar (`session.rs:467`) ve gerçek çizim boş döner.

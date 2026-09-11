# Phase 1 — Ölçüm olmayan boru ve bekçi

## Özet

Ayrı yük profili, `run_seconds`'ın üç anlamının ayrılması, atlas doluluğunun
`bt-gpu` üzerinden yeniden yayımı ve `make duman` kapısına **üst sınır**.
Hiçbir zaman ölçülmüyor — bu phase bekçiyi koruduğu şeyden **önce** kuruyor.

_Requirements: R1, R1.1, R1.2, R2, R2.1, R2.2, R6, R6.1_

---

## 1. `load_shell(secs)` — `smoke_shell`'in yanında, ondan ayrı

`crates/bt-core/src/session.rs`

`smoke_shell` (`session.rs:221`) ikinci müşteri **kaldıramaz**: nullary, tek
`printf` + `sleep 10` basıyor ve kendi doc'u onu `hucre=8 glif=6 kural=15`'in
ve üç sınamanın **tek sahibi** ilan ediyor. İkinci bir yük o sahipliği böler.

Ölçüm yükünün istediği şey de başka: tek atış değil, **koşu boyunca** akan
çıktı. Yani süre parametreli.

```rust
/// Ölçüm yükü: `secs` saniye boyunca kesintisiz çıktı akıtır.
///
/// `smoke_shell`'den **ayrı** ve öyle kalmalı — o, `hucre=8 glif=6 kural=15`
/// sayılarının tek sahibi ve üç sınama o sayılara bağlı. Buradaki komut
/// değişince duman sayıları oynamaz; oynarsa ayrım kaybolmuş demektir.
///
/// Viewport kaydırma **yok** (tekerlek işleyicisi yok), o yüzden kaydırılan
/// şey viewport değil **içerik**: her satır kirli düşer, grid yukarı kayar,
/// kare akışı kendiliğinden sürer. Ölçtüğümüz şey zaten bu — dolu bir karede
/// parse + `frame()` + encode + GPU maliyeti.
pub fn load_shell(secs: u64) -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        vec![
            "-c".to_owned(),
            // `seq` değil `while`: sabit satır sayısı makineye göre ya erken
            // biter ya da hiç bitmez. Süre kapısı deterministik.
            format!(
                "end=$(($(date +%s) + {secs})); \
                 while [ $(date +%s) -lt $end ]; do \
                   printf '%s\\n' \
                   'bateri olcum yuku 0123456789 abcdefghijklmnopqrstuvwxyz'; \
                 done"
            ),
        ],
    )
}
```

> **`smoke_shell`'in doc'una bir cümle eklenir:** "ölçüm yükü için
> `load_shell`; bu fonksiyon duman sayılarının sahibi olduğu için ikinci bir
> yük **buraya eklenmez**." Yoksa bir sonraki okuyucu doğal olarak buraya
> parametre eklemeye kalkar.

---

## 2. `Workload` — `run_seconds.is_some()`'ın üç anlamı ayrılır

`crates/bt-shell/src/lib.rs` ve `app.rs`

Bugün `run_seconds.is_some()` **üç ayrı şey** demek:

| yer | anlam |
|---|---|
| `app.rs:327` | sabit shell kullan |
| `app.rs:203` (`performSelector runDeadline:`) | deadline kur |
| `app.rs:399` (`shutdown`) | bekçiyi kur |
| `app.rs:243` | rapor bas |

Yük seçimi bunlardan yalnız birincisini ilgilendiriyor. Ayrılmazsa ölçüm
koşusu ya deadline'ı ya bekçiyi kaybeder.

```rust
/// Duman ve ölçüm koşularının shell'i. Kullanıcının `$SHELL`'i **değil**:
/// sonuç rc dosyasına bağlı olmasın.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// `make duman`: tek atış, sonra boşta. `hucre`/`glif`/`kural` sayılarının
    /// kaynağı ve boşta sıfır karenin bekçisi (bkz. madde 4).
    Smoke,
    /// `BT_SCROLL_TEST`: koşu boyunca akan çıktı.
    Load,
}

pub struct Options {
    /// `BT_RUN_SECONDS`: dolunca kare sayısına bakıp çıkılır (`make duman`).
    pub run_seconds: Option<u64>,
    /// Hangi sabit shell. `None` → kullanıcının kendi `$SHELL`'i.
    pub workload: Option<Workload>,
}
```

`app.rs:327` yeni hâli — dallanma artık yükü soruyor, süreyi değil:

```rust
command: self.ivars().workload.map(|w| match w {
    Workload::Smoke => smoke_shell(),
    // Yükün süresi deadline'la aynı olmalı: kısa kalırsa pencere koşunun
    // kuyruğunda boşa düşer ve ölçüm boşta kare örnekler.
    Workload::Load => load_shell(self.ivars().run_seconds.unwrap_or(0)),
}),
```

`main.rs` env'i tek yerde okumaya devam eder:

```rust
// `BT_SCROLL_TEST` yükü seçer, süreyi değil — süre `BT_RUN_SECONDS`'ta.
// Yük istenip süre verilmezse koşu hiç bitmez: bu bir kullanım hatası,
// sessizce sıfır saniyelik yüke düşmez.
let workload = match (std::env::var_os("BT_SCROLL_TEST").is_some(), run_seconds) {
    (true, None) => {
        eprintln!("bateri: BT_SCROLL_TEST, BT_RUN_SECONDS olmadan anlamsız");
        return ExitCode::FAILURE;
    }
    (true, Some(_)) => Some(bt_shell::Workload::Load),
    (false, Some(_)) => Some(bt_shell::Workload::Smoke),
    (false, None) => None,
};
```

> **Geriye uyum:** `BT_RUN_SECONDS` tek başına verildiğinde eskisi gibi
> `Smoke` seçiliyor, yani `make duman` hiç değişmeden çalışır.

---

## 3. Atlas doluluğu — `bt-gpu` yeniden yayımlar

`crates/bt-gpu/src/renderer.rs`

`Atlas::occupancy()` (`bt-atlas/src/lib.rs:316`) **zaten var** ve bugün yalnız
`#[cfg(test)]` altında okunuyor — dokuz kullanımın dokuzu da sınama.

Tek engel katman: `bt-shell`'in `bt-atlas` kenarı **yok**
(`crates/bt-shell/Cargo.toml` yalnız `bt-gpu` ve `bt-core`). Kenar
**eklenmez** — `Renderer::cell_metrics` (`renderer.rs:253`) zaten bu sorunun
çözülmüş hâli: `bt-atlas`'tan gelen bir değeri `bt-shell`'e katman yönünü
bozmadan taşıyor.

```rust
/// Atlasın yuva doluluğu: (kullanılan, toplam).
///
/// `cell_metrics` ile aynı gerekçe: `bt-shell`'in `bt-atlas` kenarı yok ve
/// olmamalı. Değer `bt-atlas`'ta doğuyor, `bt-gpu` yeniden yayımlıyor.
pub fn atlas_occupancy(&self) -> (usize, usize) {
    self.atlas.occupancy()
}
```

---

## 4. `yuva=` jetonu ve **duman kapısının üst sınırı**

`crates/bt-shell/src/app.rs` → `report_and_exit`

İki değişiklik, ikincisi bu phase'in asıl sebebi.

**`yuva=U/T` jetonu** 003 #3 ve 004 #2 iddialarını kapatıyor — ikisi de
zamanlama değil sayaç sorusuydu ve aracı zaten vardı.

**Kapı üst sınır kazanır.** Bugün kapı `n > 0 && k > 0 && g > 0 && r > 0`
(`app.rs:440`). Bu, sıfırı görüyor ama **fazlayı görmüyor** — oysa boşta sıfır
kareyi bozan bir değişikliğin belirtisi tam olarak fazla karedir ve
`CLAUDE.md`'nin dediği gibi "belirti sessizdir: uygulama çalışır, pil gider".

Bekçiyi icat etmeye gerek yok, **zaten var ve zaten yeşil**: `smoke_shell` bir
kez basıp `sleep 10` yapıyor, `BT_RUN_SECONDS=3` ise pencere ~3 saniye boşta
duruyor ve çıktı `kare=1`.

```rust
// Boşta sıfır karenin bekçisi. `Smoke` yükünde pencere ilk çizimden sonra
// ~`run_seconds` saniye boşta: bugün `kare=1`. Üst sınır cömert (yeniden
// boyutlama ve ölçek değişimi birkaç meşru kare daha üretebilir) ama
// kaçırmayacak kadar dar: boşta sıfır kare bozulursa 60 Hz'de 3 saniye
// ~180 kare demek. 8 ile 180 arasında tartışma yok.
//
// `Load` yükünde üst sınır YOK — orada kare akışı işin kendisi.
const IDLE_FRAME_LIMIT: u64 = 8;
```

> **Neden bu phase'de, ölçümden önce?** Bekçi, koruduğu şey geldiğinde zaten
> silahlı olsun diye. Phase-2 zaman yakalamayı ekliyor ve enstrümantasyonun en
> olası hatası tam da boşta kare üretmek (`context.md` üç yolunu sayıyor).
> Bekçi sonra kurulsaydı, ilk koşuda bozulan şeyi kimse görmezdi.

---

## Uygulama Notları

- **`atlas` alanı `RefCell<Option<AtlasTexture>>`.** Kılavuzun
  `self.atlas.occupancy()` satırı derlenmez; gerçek hâli
  `self.atlas.borrow().as_ref().map_or((0, 0), |tex| tex.atlas.occupancy())`.
  `None` bir hata değil doğru cevap: atlasın anahtarı pencereden geliyor ve
  metrik hiç sorulmadıysa açılmış yuva da yok. `expect` olmadı, çünkü burası
  `report_and_exit` yolunda ve kapanışta bir panik raporun kendisini yutardı.

- **`smoke_shell_counts_unchanged` diye yeni bir sınama yazılmadı.** `8/6/15`
  iddialarının pin'i zaten `session.rs`'teki üçlü
  (`smoke_shell_yields_background_cells`, `smoke_shell_yields_six_glyphs`,
  `smoke_shell_distinguishes_five_styles`) ve üçü de bu sette dokunulmadan
  geçti. Dördüncü bir sınama aynı sayıları yeniden türetirdi, yani **hiçbir
  koşulda kırmızı düşemezdi**. "Ayrılık" yarısını `load_shell_carries_duration`
  içindeki `assert_ne!(args, smoke_shell().1)` taşıyor. Kutu `[x]`: iddia
  korunuyor, ama koruyanı mevcut üçlü.

- **`Load` yükünün alt sınırı ayrıldı** (kılavuzda yoktu, ilk koşuda çıktı).
  `load_shell` düz metin akıtıyor: arka plan da kural da **yapısal olarak
  sıfır**. Dört sayacın dördünü de soran kapı her ölçüm koşusunda kırmızı
  düşüyordu (`kare 3, hücre 0, glif 1836, kural 0`). `verdict` (eski adı `gate_passes`) artık yükü
  soruyor: `Load` → `kare > 0 && glif > 0`, üst sınır yok; `Smoke` (ve yüksüz)
  → dördü de `> 0` **ve** `kare <= IDLE_FRAME_LIMIT`. Hata iletisinin
  gereklilik metni de yüke göre — "dördü de >0 olmalı" `Load`'da okuyanı var
  olmayan bir arızayı aramaya gönderirdi.

- **Bekçi silahlı ama `make duman` koşumunda kör — ölçüldü.** Üç gözlem:
  1. Koşu sonunda pencere `isVisible() = true` ama
     `occlusionState` `Visible` bitini **taşımıyor** (ham değer `8192`,
     `Visible = 1<<1`). `windowDidChangeOcclusionState:` hiç ateşlemiyor
     (durum hiç *değişmiyor*), yani bizim `Gate.open`'ımız `true` kalıyor —
     kareyi kısan bizim kapımız değil, sistemin display link'i askıya alması.
  2. `BT_SCROLL_TEST=1` ile `BT_RUN_SECONDS` 3, 6 ve 10 → **hep `kare=3`**.
     Bu bir hız değil, bir doyma: hasar sürekliyken bile kare sayısı artmıyor.
     Yük 260 kat hızlandıktan sonra (aşağıdaki `/simplify` maddesi) **aynı
     sonuç**: `kare=3`, `glif` 816'dan 1836'ya çıktı. Yani doyma cılız yükün
     değil, örtülü pencerenin sonucu.
  3. Boşta sıfır kare bilerek bozulduğunda (`needs_update`'in sonuna koşulsuz
     `iv.waker.wake()`) `make duman` **`kare=3` basıp geçti** — `8` sınırının
     altında.

  4. Sınır **8'den 2'ye indirildi** — kılavuzun sayısı değil, ölçümün sayısı.
     `8`, "bozulursa 60 Hz'de üç saniye ~180 kare" türetimine dayanıyordu ve o
     türetim yukarıdaki üç gözlemle çürüdü: tavan 3, yani `8` bu koşumda
     **hiçbir zaman** ateşleyemezdi. `2` üç sayının arasındaki tek anlamlı
     yer — sağlam koşuyu bir kare payla geçirir, bozulmuşu yakalar.
     **Doğrulandı:** sağlam koşu beş kez üst üste `kare=1` + çıkış 0;
     sabotajlı koşu `bateri: boşta sıfır kare bozuldu — 3 saniyelik koşuda
     3 kare çizildi, üst sınır 2` + çıkış 1. Bekçi artık gerçekten koruyor.

  Kapının koştuğu **tek** bağlam `make duman`'dır: `report_and_exit` yalnız
  `BT_RUN_SECONDS` yolunda çalışıyor, yani etkileşimli koşu bu sınırı hiç
  değerlendirmiyor — "pencere görünürken zaten gerçek" savunması bu yüzden
  geçersizdi. `.app` paketi (`make kur`) gelince görünür pencerede tavan
  kalkar ve sınır **yeniden ölçülmelidir**; bu not sabitin doc'unda da duruyor.

- **Escalation değil, gerekçesi:** R6.1 **ölçülmüş bir sınırla** teslim
  edildi ve `kare=1 hucre=8 glif=6 kural=15` bit bit korundu. Geçersiz olan
  gereksinim değil, §4'ün `8`i türeten varsayımıydı; çürütülmüş bir türetimi
  ölçülmüş bir sayıyla değiştirmek uygulamadır, yeniden tasarım değil — ve
  alternatif (kılavuzun `8`ini olduğu gibi göndermek) ateşleyemeyen bir bekçi
  göndermek olurdu, yani phase'in var olma sebebini boşa çıkarmak. Sapma
  kayıtlı, üç sayı da yazılı, `.app` gelince yeniden ölçüm notu düşüldü.

- **code-reviewer'ın L3'ü (not, düzeltme değil):** `verdict`'in
  `Some(Workload::Smoke) | None` kolu, kullanıcının kendi `$SHELL`'iyle koşan
  bir harness'a da duman reçetesinin dört sayacını **ve** üst sınırı
  uyguluyor. `main.rs`'ten ulaşılamaz; kalıcı çözüm `Options`'ın tek alana
  inmesi ve o phase-2'ye devredildi.

## Yayın Etkisi

- **Duman sözleşmesi:** jeton **eklendi, silinmedi** — `yuva=U/T` ve
  `yuk=smoke|load`. Okuyan
  taraf tanımadığı jetonu atlayabilir; `kare`/`hucre`/`glif`/`kural` aynen
  duruyor ve **sayıları değişmiyor** (`load_shell` yalnız `BT_SCROLL_TEST`
  ile devreye giriyor, `make duman` `Smoke` koşuyor).
- **`CLAUDE.md` ve `proje.md`:** `make duman` satırı `yuva=` jetonunu ve
  kapının yeni üst sınırını anlatacak şekilde güncellenir.
- Yeni bağımlılık **yok**, `Cargo.lock` oynamamalı. `.metal`, `build.rs`,
  `assets/`, ayar şeması, tema, shell entegrasyonu: el değmiyor.
- **Ölçüm bekliyor: yok.** Bu phase iki iddia **kapatıyor** (003 #3, 004 #2 —
  atlas doluluğu) ve hiçbir yeni iddia doğurmuyor: eklenen tek iş bir erişimci
  çağrısı ve bir karşılaştırma, ikisi de kapanış yolunda. Duman koşusunda
  ölçülen doluluk: `yuva=13/2048`.
- **Doğrulanan duman satırı (2026-09-11, debug profili):**
  `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke pipeline=ok`,
  çıkış 0, art arda beş koşuda da aynı. Dört sayaç da öncekiyle bit bit aynı;
  eklenen iki jeton `yuva=` ve `yuk=`, silinen jeton yok.
- **Ölçüm yükünün satırı** (`BT_SCROLL_TEST=1 BT_RUN_SECONDS=3`):
  `kare=3 hucre=0 glif=1836 kural=0 yuva=37/2048 yuk=load pipeline=ok`,
  çıkış 0.
- **`proje.md` → `make duman` satırı** da güncellendi: `yuva=` jetonu, üst
  sınır ve üst sınırın kendi sınırı (örtülü pencerede kare sayısının doyması)
  oraya yazıldı. `Makefile`'ın `duman` yorumu aynı üç şeyi taşıyor.

---

## Checklist

- [x] `load_shell(secs)` — `bt-core`, `smoke_shell`'in yanında; `smoke_shell`'in doc'una "ikinci yük buraya eklenmez" cümlesi
- [x] `Workload` enum'ı + `Options.workload`; `app.rs:327` dallanması yükü sorar, süreyi değil
- [x] `main.rs`: `BT_SCROLL_TEST` okunur; süresiz yük **kırmızı düşer**, sessizce sıfıra inmez
- [x] `Renderer::atlas_occupancy()` — `cell_metrics` deseni; `bt-shell`'e `bt-atlas` kenarı **eklenmedi**
- [x] `yuva=U/T` jetonu `report_and_exit`'e eklendi
- [x] Duman kapısına `IDLE_FRAME_LIMIT` — yalnız `Smoke` yükünde (kılavuzun `8`i ölçümle `2`ye indi, bkz. Uygulama Notları)
- [x] Test: `load_shell_carries_duration` → komut `run_seconds`'ı içeriyor ve `smoke_shell`'den farklı
- [x] Test: `smoke_shell_counts_unchanged` — **yeni sınama yazılmadı**, pin mevcut üçlü (`smoke_shell_yields_background_cells` / `_six_glyphs` / `_distinguishes_five_styles`); üçü de dokunulmadan geçti ve `make duman` `hucre=8 glif=6 kural=15` bastı. Gerekçe: Uygulama Notları
- [x] Test: `atlas_occupancy_is_republished` → `Renderer::atlas_occupancy` `Atlas::occupancy` ile aynı çifti veriyor
- [x] Test: `idle_limit_catches_excess_frames` — sınır karşılaştırması saf fonksiyon olarak sınanır (gerçek display link gerektirmeden)
- [x] Doğrulama geçti (`make hepsi` → 0; `make duman` → `kare=1 hucre=8 glif=6 kural=15 yuva=13/2048 yuk=smoke pipeline=ok`, çıkış 0, beş koşuda da aynı; `make test-yaris` → 0. `make shader` ve `make terminfo` **gerekmedi**: `.metal`, `build.rs` ve `assets/terminfo` el değmedi)
- [x] `/simplify` çalıştırıldı (dört mercek ajanı), bulgular uygulandı; uygulanmayan üçü gerekçesiyle Uygulama Notları'nda
- [x] `/code-review` çalıştırıldı, beş bulgunun beşi de giderildi. Skill fork'u ~35 dk sessiz kaldı, bu sırada `proje.md` basamak 2 uygulanıp `code-reviewer` subagent'ı da koşturuldu; ikisi de aynı bulgularla döndü (subagent ayrıca M2 `u64::MAX` sarması ve M3 `pub Options` sessiz-yeşil dallarını buldu, ikisi de düzeltildi)
- [x] `/audit` çalıştırıldı: mekanik mercekler (1, 2, 3, 6) inline ve temiz; 4, 5, 9 ilgisiz; yargı mercekleri ajanla — 7 temiz, 8 sınırın ateşleyemediğini yakaladı (8→2), 10 dört belge çelişkisi buldu, hepsi giderildi
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `9788d95`

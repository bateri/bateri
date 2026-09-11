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
const BOSTA_UST_SINIR: u64 = 8;
```

> **Neden bu phase'de, ölçümden önce?** Bekçi, koruduğu şey geldiğinde zaten
> silahlı olsun diye. Phase-2 zaman yakalamayı ekliyor ve enstrümantasyonun en
> olası hatası tam da boşta kare üretmek (`context.md` üç yolunu sayıyor).
> Bekçi sonra kurulsaydı, ilk koşuda bozulan şeyi kimse görmezdi.

---

## Uygulama Notları

## Yayın Etkisi

- **Duman sözleşmesi:** jeton **eklendi, silinmedi** — `yuva=U/T`. Okuyan
  taraf tanımadığı jetonu atlayabilir; `kare`/`hucre`/`glif`/`kural` aynen
  duruyor ve **sayıları değişmiyor** (`load_shell` yalnız `BT_SCROLL_TEST`
  ile devreye giriyor, `make duman` `Smoke` koşuyor).
- **`CLAUDE.md` ve `proje.md`:** `make duman` satırı `yuva=` jetonunu ve
  kapının yeni üst sınırını anlatacak şekilde güncellenir.
- Yeni bağımlılık **yok**, `Cargo.lock` oynamamalı. `.metal`, `build.rs`,
  `assets/`, ayar şeması, tema, shell entegrasyonu: el değmiyor.
- **Ölçüm bekliyor: yok.** Bu phase iki iddia **kapatıyor** (003 #3, 004 #2 —
  atlas doluluğu) ve hiçbir yeni iddia doğurmuyor: eklenen tek iş bir erişimci
  çağrısı ve bir karşılaştırma, ikisi de kapanış yolunda.

---

## Checklist

- [ ] `load_shell(secs)` — `bt-core`, `smoke_shell`'in yanında; `smoke_shell`'in doc'una "ikinci yük buraya eklenmez" cümlesi
- [ ] `Workload` enum'ı + `Options.workload`; `app.rs:327` dallanması yükü sorar, süreyi değil
- [ ] `main.rs`: `BT_SCROLL_TEST` okunur; süresiz yük **kırmızı düşer**, sessizce sıfıra inmez
- [ ] `Renderer::atlas_occupancy()` — `cell_metrics` deseni; `bt-shell`'e `bt-atlas` kenarı **eklenmedi**
- [ ] `yuva=U/T` jetonu `report_and_exit`'e eklendi
- [ ] Duman kapısına `BOSTA_UST_SINIR` — yalnız `Smoke` yükünde
- [ ] Test: `load_shell_sure_kapisi_tasir` → komut `run_seconds`'ı içeriyor ve `smoke_shell`'den farklı
- [ ] Test: `smoke_shell_sayilari_oynamadi` — `hucre=8 glif=6 kural=15` iddiaları bit bit aynı geçiyor
- [ ] Test: `atlas_doluluk_yeniden_yayimlaniyor` → `Renderer::atlas_occupancy` `Atlas::occupancy` ile aynı çifti veriyor
- [ ] Test: `bosta_ust_siniri_fazla_kareyi_gorur` — sınır karşılaştırması saf fonksiyon olarak sınanır (gerçek display link gerektirmeden)
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make duman` **zorunlu**: kapı değişti)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}

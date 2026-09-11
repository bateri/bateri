# VT motoru — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md)

Pencere artık gerçek bir shell çalıştırıyor: `bt-core` `alacritty_terminal`'i
kapsülleyen platformsuz çekirdeği taşıyor, `bt-gpu` hücre arka planlarını
`CAMetalDisplayLink` ritminde ve yalnız hasar varken çiziyor, `bt-shell`
klavyeyi PTY'ye akıtıp kapanışta shell çocuğunu bitiriyor. Kullanıcının
makinesinde değişen tek durum yok — ayar dosyası, tema ve terminfo bu sette
hiç doğmadı; dışarıya etki **bir yeni bağımlılık ailesi**, **değişen bir
`make duman` sözleşmesi** ve **bekleyen iki ölçüm**.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi        # rustc sürümü + fmt + clippy -D warnings + test
make shader       # .metal kanaryası (phase-2 shader ekledi)
make test-yaris   # PTY okuyucu ↔ kare üreten taraf yarış stresi
make duman        # pencereyi açar, kare ve hücre sayar
```

### Beklenen çıktı

- `make hepsi` → sessiz yeşil. Sınama sayısı: `bt-core` 12 (+1 `#[ignore]`),
  `bt-gpu` 10, `bt-shell` 6.
- `make duman` → **`kare=1 hucre=8 pipeline=ok`**, çıkış 0. `hucre=8`
  uydurma değil: duman koşusunda shell sabittir (`bt_core::smoke_shell`) ve
  aynı betiğin sekiz hücre verdiği `sabit_shell_arka_plan_hucreleri_verir`
  ile sınanıyor. Başsız ortamda (SSH, CI) binary `ATLANDI: Aqua oturumu yok`
  basıp 78 ile çıkar — bu "geçti" değildir.
- Ölçüm sayısı **yok** ve `docs/OLCUMLER.md` bu teslimle değişmiyor — ama bu
  "iddia yok" demek değil: aşağıda **iki** kare süresi iddiası ölçüm bekliyor.
  Ayrım şu: teslim bir sayı *yayımlamıyor*, çünkü ölçüm aracı yok. Sayı
  gelince buraya değil `docs/OLCUMLER.md`'ye yazılır.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make shader` yeşil (`.metal` phase-2'de değişti)
- [x] `make test-yaris` yeşil (iki zamanlama profili; TSan nightly ister,
      araç zinciri pin'li değil — bilinen sınır, waive değil)
- [x] `make duman` → `kare=1 hucre=8 pipeline=ok`, çıkış 0
- [x] `BT_RUN_SECONDS=11` (shell deadline'dan önce çıkar) → jetonlar yine
      basılıyor, çıkış 0; `ps` yetim shell bırakmıyor
- [x] Bekçi: `trap '' HUP` yiyen çocukla `_exit(70)`, jeton basılmıyor
- [x] **Göz kontrolü — kullanıcı koştu, geçti**: pencerede imleç bloğu,
      körlemesine yazılan `sh /tmp/r` ile renkli hücreler, `exit` ve kırmızı
      düğmeyle temiz kapanış, simge durumu/örtülme sonrası kare geri geliyor.
      (`ls --color` yanlış testti — `ls` ön plan basıyor, arka plan değil;
      bkz. phase-4 → Uygulama Notları)

## B. Yayın (doğrulamadan SONRA)

### B.1 Apache-2.0 attribution `[elle]`

`alacritty_terminal` Apache-2.0. Lisans metni ve attribution paneli **bundle
setinin** işi (`.app` paketi, `Info.plist`, Sparkle henüz yok) ve bu sette
kapsam dışı bırakıldı. Bundle seti açıldığında bu satır oraya taşınır;
`bateri` bugün bir `.app` olarak dağıtılmıyor.

### B.2 Ölçüm `[komut]`

İki iddia ölçüm bekliyor, ikisi de sayı **uydurulmadan** bırakıldı:

1. `Session::frame()`'in `FairMutex::lock()` beklemesi display link
   callback'inde — okuyucunun ≤64 KiB ayrıştırma lease'inin arkasında.
2. Kare başına instance tamponu ayırmanın maliyeti ve `setVertexBytes` eşiği
   (≤4 KiB); üçlü tamponlama kararı buna bağlı.

```sh
/measure 002-vt-motoru
```

Sonuç `docs/OLCUMLER.md`'ye girer — başka hiçbir belgeye sayı yazılmaz.

#### 2026-09-10 — koşuldu, sonuç: **ölçüm aracı yok**

`/measure 002-vt-motoru` koştu ve sayı **üretmedi**. İki iddia da kare süresi
ailesinden ve ikisi de aynı eksik kancaya dayanıyor:

| # | iddia | gereken kanca | durum |
|---|---|---|---|
| 1 | `FairMutex::lock()` beklemesi | `BT_FRAME_LOG` + `BT_SCROLL_TEST` | kanca yok |
| 2 | kare başına instans tamponu + `setVertexBytes` eşiği | `BT_FRAME_LOG`, `cargo bench` | kanca yok, bench hedefi yok |

Kanıt ve gerekçenin tamamı `003-glyph-atlas/teslim.md` → B.1'in aynı tarihli
notundadır (kancalar depoda yalnız belgelerde geçiyor, `cargo bench
--workspace -- --list` → `0 benchmarks`, boşta sıfır kare yüzünden profiler
geri düşüşü de yok). İki set aynı kancaları bekliyor; kanca seti açıldığında
**ikisi birden** ölçülür.

Kutu `[ ]` kalıyor — ölçüm bir kapı değil, atlanmış da değil: **aracı yok**.

### B.3 Bağımlılık kaydı `[oto]`

`Cargo.lock` bu sette **+47 paket** aldı (phase-1, ölçüldü; çoğu Windows
hedefi, macOS'ta derlenen `bt-core` ağacı 27 satır). Kaynağı tek bir
kullanıcı onaylı karar: `alacritty_terminal` (`discussion.md` → Karar 1/2).
Sonraki phase'lerin eklediği `block2`, `dispatch2` ve `libc` satırları
**yeni crate çekmedi** — üçü de grafta zaten vardı, `Cargo.lock`'ta yalnız
ilgili crate'in bağımlılık listesine kenar eklendi.

### Yayın Checklist

<!-- `/ship` bekleyen manuel adımları BU başlık altında arar. -->

- [ ] B.1 Apache-2.0 attribution `[elle]` — bundle setine devredildi, bu
      teslimde yapılacak bir şey yok
- [ ] B.2 `/measure 002-vt-motoru` `[komut]` — iki ölçüm bekliyor;
      2026-09-10'da koştu, **ölçüm aracı yok** (kanca seti bekliyor, bkz. B.2)
- [x] B.3 Bağımlılık kaydı `[oto]` — `Cargo.lock` depoda, gerekçeler
      manifest yorumlarında ve `discussion.md → Karar`'da

## Geri Alma

- **Kod:** dört phase, dört commit — `ed1c5a3`, `3191677`, `98baf87`,
  `b458d7f`. Sırayla revert edilebilir; her biri tek başına `make hepsi`
  yeşil bırakacak şekilde kesildi.
- **`make duman` sözleşmesi:** phase-3 `kare=N pipeline=ok` → `kare=N hucre=K
  pipeline=ok` yaptı. Jeton **eklendi, silinmedi**; okuyan taraf `hucre=`
  tanımıyorsa atlar, yani geri alma tek commit ve kırılan bir tüketici yok.
- **Ayar şeması / tema / terminfo:** dokunulmadı, geri alınacak bir şey yok.
- **Türetilmiş dosya:** yok — `default.metallib` `target/` altında kalır,
  depoya girmez.
- **Belgeler:** `CLAUDE.md` ve `proje.md` değişiklikleri kod commit'lerinin
  içinde; kod geri alınırsa belge de aynı commit'te geri gelir.

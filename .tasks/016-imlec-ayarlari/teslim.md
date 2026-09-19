# 016-imlec-ayarlari — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md)

İmlecin görünüşünü ve ritmini belirleyen sayılar **artık ayar dosyasında**:
köşe yarıçapı, gölgenin gücü, odaksız hâli ve blink periyodu. Dördü de
`[terminal]` altında, kayıt anında uygulanıyor ve varsayılanları bugünkü
görüntü — yani dosyası olmayan kullanıcı hiçbir fark görmüyor.

## Bedeli, açıkça

- **`bt-core` dört yeni `pub const` ve bir `pub` tip kazandı** (`CaretStyle`,
  `UnfocusedCaret`). Varsayılanların **tek sahibi** orası ve `bt-gpu` aynı
  sabitleri **import ediyor**; iki literal olsaydı piksel bekçileri kendi
  tutarlılığını sınar, sevk edilen imleç başka ölçüde olsa da yeşil geçerdi.
- **`CursorMotion`'ın yazılı kuralı bilerek delindi:** *"sayıların ayar
  modelinde durması onları iki yerden değiştirilebilir kılardı."* Kullanıcı
  sayıların kendisini istedi; delik tek yerde ve gerekçesi `discussion.md`'de.
- **Kapı bozuk bir blink periyodunu göremiyor.** Süreli koşu ayar dosyasını
  hiç okumuyor ve blink varsayılanı kapalı, yani `0.05` bir periyot
  `sessiz=` katını hiçbir koşulda kızartmaz. Tek koruma kabul aralığı.

## Kapsam

Panel üç mercekle koştu ve kapsam **daralarak** çıktı: beş anahtar yerine üç
(hale payı + alfası tek çarpana indi, `IDLE_STOP` çıkarıldı). Dördüncü anahtar
(`cursor_unfocused`) sonradan **kullanıcı isteğiyle** eklendi ve kaydı
`discussion.md` → Karar 6'da; ölçüm o davranışın kusur değil **zevk** olduğunu
gösterdikten sonra.

**Opacity/fade kapsam dışı** ve gerekçesi `context.md`'de: geçiş demek ekran
hızında kare demek ve `QUIET_FLOOR` ile çarpışma gerçek olurdu.

## Kapı

**`/code-review`** (set aralığı): **11 bulgu, hepsi giderildi.** Biri
kullanıcıya görünüyordu — `CaretStyle` alanları `f32`'ken tanı metni geri
düşülen değeri `f64`'e genişletip basıyordu (`using 0.10000000149011612`).
Alanlar `f64` oldu ve tanının **tam metnini** sınayan bir bekçi eklendi;
kardeş sınamalar mesajın tamamına bakıyordu, bu anahtar yalnız `key`'e.

**İki doc çalınmıştı** (`named_enum` ↔ `cursor_blink`, `set_caret_style` ↔
`set_focused`) ve bu aynı hatanın **dördüncü** tekrarı — 015'te de iki kez
olmuştu. Yol haritasına borç olarak yazıldı: `clippy` bunu ancak araya boş
satır girerse görüyor.

Kalanlar: `UnfocusedCaret`'in adları iki yerdeydi (tam da `named_enum`'un
emekli etmek için doğduğu borç), `HALF_PERIOD` gerekçeyi tekrarlıyordu,
`caret_focused` üretimde ölü alandı, açılış ve kayıt anı iki ayrı liste
tutuyordu (`apply_caret`'te birleşti), gölge tavanının gerekçesi yalnız alfa
eksenini sayıyordu.

**`/audit`**: `make denetim` **temiz**; bağımlılık ve shader/düzen mercekleri
**ilgisiz**. Bir bulgu: `docs/AYARLAR.md`'nin bölüm örneği ve prozası yeni
anahtarları anmıyordu — ve tablo satırları **hiç inmemişti** (`replace`
sessizce tutmamış, doğrulanmamıştı).

## A. Doğrulama

```sh
make hepsi          # exit 0
make test-yaris     # exit 0
```

### Doğrulama Checklist

- [x] `make hepsi` yeşil — her phase'de ve kapıda **0**
- [x] `make test-yaris` yeşil
- [x] `/code-review` — 11 bulgu, hepsi giderildi
- [x] `/audit` — `make denetim` temiz, 1 bulgu giderildi
- [x] **Gözle kontrol** — kullanıcı yaptı (2026-09-20): dört anahtar da
      kayıt anında uygulanıyor
- [ ] `make duman` — **kullanıcıda** (aşağıda B.1)

### Ölçüm bekleyen iddia

- **"Periyot ucuz."** Yön koddan kanıtlı (ekran hızına çıkmıyor, bedel
  doğrusal), **büyüklüğü ölçülmedi**. Bu set kanca doğurmadı.

## B. Yayın

### B.1 `make duman` `[komut]`

```sh
make duman
```

Ajanın kabuğunda yanlış tanıyla kırmızı düşüyor; gerçek pencere istiyor.
**Jetonlar değişmemeli**: süreli koşu ayar dosyasını hiç okumuyor, yani bu
setin hiçbir anahtarı hermetik koşuya dokunmuyor. Değişirlerse bir yol yanlış
kurulmuş demektir.

### Yayın Checklist

- [ ] B.1 `make duman`
- [ ] `/ship`

`make kur` **gerekmiyor**: kabuk betiği, terminfo, jeton satırı ve app bundle
değişmedi.

## Göç

**Yok ve bu bir tuzak taşıyor.** Yeni anahtarlar opsiyonel ve varsayılanları
bugünkü görüntü, yani dosyası olan kullanıcı hiçbir fark görmüyor — **ama yeni
anahtarları da görmüyor.** `Settings::create_if_missing` yalnız dosya
**yokken** yazıyor; var olan dosyaya yazan tek yol View ▸ Theme ▸ ve o da
yalnız `[appearance] theme`'i değiştiriyor.

Bu sette kullanıcının dosyası **iki kez elle** tazelendi (şablondan yeniden
üretilip değerleri korunarak). Kalıcı çare `docs/YOL-HARITASI.md`'ye borç
olarak yazıldı: var olan dosyaya yazmak **yeni bir yazma yolu**, yani mimari
karar.

## Bilinen sınırlar

- **Kapı bozuk bir blink periyodunu göremiyor** (yukarıda). Tek koruma aralık.
- **`docs/AYARLAR.md`'nin bölüm örnekleri hiçbir sınamayla bağlı değil** —
  `### Şablon` bloğu çivili, `## Anahtarlar` altındakiler serbest ve bu sette
  sessizce bayatladı. Borç yazıldı.
- **Doc yorumunun altına kod sokmak sessizce doc çalıyor** — 015 ve 016'da
  dört kez oldu ve mekanik bir kapısı yok. Borç yazıldı.
- **Enum ayrıştırıcısının beş kopyası duruyor.** `named_enum` doğdu ama beş
  kopya taşınmadı: her birinin tanı cümlesi kendi sözcükleriyle yazılı.
  Ondalık tarafta `ranged_float` doğdu ve `line_height` **taşındı**;
  `font_size` tek uçlu olduğu için kaldı.
- **`cursor_unfocused = "solid"` blink'e dokunmuyor:** odaksız pencerede blink
  her hâlde duruyor. İkisi ayrı sinyal ve bu bilinçli.

## Geri Alma

- **Kod:** phase commit'lerini revert. Üçü bağımsız — yüzey ikilisi
  (phase-1), blink periyodu (phase-2) ve odaksız hâl (phase-3).
- **Ayar şeması:** anahtarlar **silinmez**; geri alınırsa emekli edilir
  (dosyada korunur, okunmaz, görülünce tanı) — 009'un `prompt` anahtarı emsali.
- **Varsayılanlar:** hepsi bugünkü görüntü, yani geri alma kullanıcının
  gördüğünü değiştirmiyor.
- **Belge:** `CLAUDE.md`, `docs/AYARLAR.md`, `docs/YOL-HARITASI.md` aynı
  commit'lerde.

## Sonraki iş

- **017 klavye** — Option+oklar kelime atlama, Option+Delete kelime silme,
  Cmd+Delete satır silme. Yol haritasında sırada ve kullanıcının günlük isteği.
- **Tamamlama listesi ızgarayı kaydırıyor** — 015 sırasında teşhis edildi ve
  ölçüldü.
- **Var olan ayar dosyasının tazelenmesi** — bu setin doğurduğu borç.

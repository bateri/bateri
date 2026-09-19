# 015-imlec-cilasi — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md)

İmleç artık **doğru anda kıpırdıyor ve iyi görünüyor**. Hızlı komutta caret'in
dock'tan ızgaraya çıkıp geri inmesi kalktı (histerezis), caret `cell_bg`'nin
düz dörtgeninden çıkıp **kendi fragment'ine** taşındı — köşesi yuvarlak ve
çevresinde hafif bir gölge var — ve odakta olmayan pencerede **içi boşalıyor**,
blink duruyor. Dışarıya görünen tek şey davranış: ayar anahtarı yok, tema
biçimi değişmedi, jeton satırı aynı.

## Bedeli, açıkça

- **`bt-gpu` üçüncü pipeline'ını kazandı.** Vertex paylaşılıyor
  (`cell_bg_vertex`), ayrılan yalnız fragment; gerekçe SDF hesabını kare başına
  binlerce arka plan dörtgenine ödetmemek.
- **`bt-gpu`, `Cursor::blink`'in ikinci sahibi oldu** (R7.4, kullanıcı kararı).
  Karşılığı boşta sıfır kare tarafında bir kazanç: odaksız pencere saat
  kurmuyor.
- **Üç seçilmiş sayı** (`HANDOVER_HOLD`, `CARET_RADIUS_RATIO`,
  `CARET_GLOW_RATIO` + `CARET_GLOW_ALPHA`); üçü de `const` doc'unda "seçilmiş,
  ölçülmemiş" ve gerekçeli. İkisi **gözle iki tur indi** (kullanıcı: "bu nasıl
  shadow pavyona döndü", ardından "radius'u da azalt").

## Kapı

**`/code-review` iki kez koştu** — phase-2 riskliydi (`make shader`) ve set
sonunda aralığın tamamı. **Toplam 27 bulgu; 26 giderildi, 1 bilinen sınıra.**
Altısı gerçek kod ya da bekçi kusuruydu:

- **Odak hiç tohumlanmıyordu:** `windowDidBecomeKey:` link yuvaya girmeden
  düşüyor ve atılıyor; arka planda açılan pencere odaksız olduğu hâlde dolu
  caret çizip blink saatini kurardı.
- **İçi boş caret sıfır kural metriğinde doluyordu:** kararın iki yarısı aynı
  tabanı görmüyordu; ters çevirme kalkmışken caret opak çizilirdi.
- **`move_caret` odağı kayıplı türetiyordu** (ince şekiller hiç boşalmıyor).
- **Köşe sınamam totolojiydi:** referansı caret'in kendi simetrik köşesinden
  alıyordu, hiçbir yarıçapta düşemezdi. Düzeltilip üretim oranıyla koşunca
  köşenin %69 boyalı olduğu çıktı.
- **İçi boş caret kolu ölü sevk ediliyordu** (shader'da yazılıydı, hiç
  koşmamıştı); phase-3 onu açmadan önce sınandı.
- **Tutmanın kare istediğini hiçbir şey sınamıyordu** ve sınaması yazıldıktan
  sonra bağlayan satır silinerek kızartıldı.

Kalan yirmi bulgu yapı ve belge: kopyalanan çizim gövdesi, kopyalanan piksel
yardımcıları, yarıçap kırpmasının üç yazarı, **kod araya girerek çalınan iki
doc bloğu** (`set_reduce_motion` ve `apply_reduce_motion` doc'suz kalmıştı) ve
bir düzine bayat cümle.

**`/audit`**: `make denetim` **temiz**. Bağımlılık ve ayar/tema mercekleri
**ilgisiz**. İki bulgu: `apply_focus` hermetik kapısı her odak değişiminde ev
dizinini çözüyordu, ve ölçülmüş iki sayı (`44 ms`, `230 ms`) `CLAUDE.md` ile
yol haritasına kopyalanmıştı — tek sahip `docs/OLCUMLER.md`.

## A. Doğrulama

```sh
make hepsi          # exit 0
make shader         # exit 0  (.metal değişti)
make test-yaris     # exit 0  (ShellLog'a iki yeni alan)
```

### Doğrulama Checklist

- [x] `make hepsi` yeşil — her phase'de ve kapıda **0**
- [x] `make shader` yeşil (phase-2 `.metal`'i açtı)
- [x] `make test-yaris` yeşil (phase-1 `ShellLog`'a alan ekledi)
- [x] `/code-review` — phase-2'de ve set aralığında; 27 bulgu, 26 giderildi
- [x] `/audit` — `make denetim` temiz, 2 bulgu giderildi
- [ ] `make duman` — **kullanıcıda** (aşağıda B.1)
- [ ] **Gözle kontrol** — **kullanıcıda** (aşağıda B.2)

### Ölçüm bekleyen iddia

- **"Sıçrama azaldı."** İddia **azaltma**, kaldırma değil (R1.4): tutma
  animasyonun yerleşme süresinin altında, yani onu aşan komutlarda belirti
  küçülür ama bitmez. Doğrulaması önce/sonra göz kontrolü; kancası yok.
- **"Odaksız pencere pil yakmıyor."** Yön koddan kanıtlı (saat kurulmuyor),
  **büyüklüğü ölçülmedi**. Bu set kanca doğurmadı.

## B. Yayın

### B.1 `make duman` `[komut]`

```sh
make duman
```

Ajanın kabuğunda **yanlış tanıyla** kırmızı düşüyor; gerçek pencere istiyor.
Beklenen: `kare`, `hucre`, `glif`, `kural`, `hareket` > 0; `icerik` ≤ 8;
`sessiz` ≥ 868 ms; `kapanis=clean`.

**Koşarken başka bir pencereye geç.** Odak kapısı hermetik koşuda hiç
okunmuyor (R7.1) ve bunun tek görünür kanıtı bu: geçiş kapıyı kırmızıya
düşürmemeli.

### B.2 Gözle kontrol `[elle]`

`cargo run -q -p bateri` ile:

1. **Sıçrama.** `ls` yaz, Enter → caret **hiç kıpırdamamalı**. `sleep 2` →
   caret tutma kadar sonra ızgaraya çıkmalı, komut bitince dock'a dönmeli.
2. **Yüzey.** Köşe yuvarlaklığı ve gölge; ikisi de "hafif dokunuş"
   ölçeğinde mi. Punto değiştir (Cmd +/−) → üçü de ölçeklenmeli.
3. **Odak.** Başka pencereye geç → caret'in **içi boşalmalı** ve blink
   durmalı, ama caret **görünür kalmalı**. Geri dön → dolmalı.
4. **Dock.** Dock'ta yazarken hepsi aynı (caret tek).
5. **İnce şekiller.** `printf '\e[5 q'` (beam) → odaksızda **şekil
   değişmemeli**, yalnız blink durmalı. Bu bilinen sınır, kusur değil.

### Yayın Checklist

- [ ] B.1 `make duman` (başka pencereye geçerek)
- [ ] B.2 gözle kontrol — beş madde
- [ ] `/ship`

`make kur` **gerekmiyor**: kabuk betiği, terminfo, jeton satırı, ayar şeması
ve app bundle değişmedi.

## Bilinen sınırlar

- **Ters çevirme dikdörtgeni keskin, boyanan alan yuvarlak.** Köşede caret
  boyamıyor ama `cell.metal` o pikseli hâlâ `Cursor::text` ile çiziyor.
  **Varsayılan zeminde görünmez** (o renk temanın zemini, yani zemine
  karışıyor); hücrenin zemini farklıysa (seçim, SGR arka planı) köşede bir
  çentik bırakır. Çaresi `cell.metal`'e SDF taşımak — Karar 2'yi yeniden açar.
- **Hücrenin kenarına mürekkep koyan glyph, içi boş caret'in halkasının
  üstüne çiziliyor.** Çizim sırasının gerekçesi (blok opak, altındaki harf ters
  çevrilmiş renkle) içi boş caret'te iki yarısıyla birden düşüyor. Bugün seyrek
  — kutu çizim henüz yok — **018'de görünür olacak**.
- **Halenin üstü ilk içerik satırında kırpılıyor** (viewport `origin_px`'ten
  başlıyor). R9 bunu baştan sayıyordu; hale sönükleştikten sonra belirti de
  sönük.
- **`cursor_motion = "snap"` ve Hareketi Azalt tutmanın bedelini ödüyor,
  karşılığını almıyor:** o kiplerde korunacak bir uçuş yok.
- **`CORRECT`'in `[nyae]`'i ve `zle -M`** `line-finish` ile aynı yüklem
  durumu, yani caret onlarda da tutma kadar geç geliyor. Satır ızgarada
  görünüyor; geciken yalnız caret.
- **`set_focused`'ın no-op'u ve blink'in `AND`'i birim sınamayla çivilenemedi**
  (`DisplayLink` sınamalarda kurulamıyor). Emsal `set_reduce_motion`, onun da
  sınaması yok. İki `[~]` kutusu phase-3'te.

## Geri Alma

- **Kod:** phase commit'lerini revert. Üçü bağımsız — odak (phase-3) tek
  başına, yüzey (phase-2) tek başına, histerezis (phase-1) en altta.
- **Dejenere kol desteklenen ve sınanan bir hâl** (R8): yarıçap 0 + hale 0
  çıktıyı 014'ün düz dörtgeniyle **bit bit** aynı yapıyor, yani yüzeyi
  commit'e dokunmadan da kapatmak mümkün.
- **Odak:** `focused` varsayılanı `true`, yani phase-3 revert edilirse kalan
  kod bugünkü davranışa döner.
- **Ayar şeması / tema / terminfo / app bundle:** dokunulmadı, geri alınacak
  bir şey yok.
- **Belge:** `CLAUDE.md`, `bt-gpu/src/lib.rs` başlığı ve `docs/YOL-HARITASI.md`
  aynı commit'lerde; revert onları da geri alır.

## Sonraki iş

- **016 klavye** — Option+oklar kelime atlama, Option+Delete kelime silme,
  Cmd+Delete satır silme. Yol haritasında sırada ve kullanıcının günlük
  isteği.
- **Tamamlama listesi ızgarayı kaydırıyor** — bu set sırasında teşhis edildi ve
  ölçüldü; çaresi listeyi ızgaraya hiç düşürmemek. `docs/YOL-HARITASI.md` →
  Sete bağlanmamış borçlar.
- **OSC 12** (uygulamanın imleç rengini değiştirmesi) 014'ten beri açık.

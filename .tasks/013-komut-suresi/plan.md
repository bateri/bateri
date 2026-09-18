# Komut süresi sayacı

## Hedef

Izgarada komut satırının **en sağında**, bir saniyeyi geçen komutların süresi
görünsün: koşarken saniyede bir ilerleyen canlı bir sayaç, bittikten sonra
sabit son süre. Bir saniyenin altındakiler hiç görünmesin.

## Gereksinimler

- **R1 — Eşik.** Süre yalnız komut bir saniyeyi geçtiyse görünür; altındakiler
  ne koşarken ne bittikten sonra sayaç doğurur.
- **R2 — Canlı.** Koşan komutun sayacı, PTY'den bayt gelmese bile
  (`sleep 5`) saniyede bir ilerler.
- **R3 — Durur.** Komut bitince sayaç son değerinde donar ve o değeri
  scrollback'te taşır; hiçbir kare daha istenmez.
- **R4 — Örtmez.** Sayaç kullanıcının komut metnini hiçbir koşulda örtmez.
- **R5 — Boşta sıfır kare korunur.** Komut koşmuyorken kare sayısı bugünkünden
  bir fazla olmaz; saat armed değilken hiç okunmaz.
- **R6 — Entegrasyonsuz oturum etkilenmez.** OSC 133 gelmeyen kabukta
  (`/bin/sh`, `integration = "off"`) blok yok, sayaç yok, saat yok.

## Yaklaşım

**Süre defterde, sayaç hücrede, saat link'te.**

- **Defter.** `Outcome::Finished` çıkış kodunun yanına geçen süreyi de alır
  (`elapsed_ms: u32`). Başlangıç anı bloğun içinde değil `ShellLog`'da tek bir
  alanda durur (`running_since: Option<Instant>`) — aynı anda **tek** komut
  koşuyor, yani 10 000 girdinin her birine bir `Instant` koymak sekme başına
  ödenen bir bedel olurdu.
- **Sayaç.** Yeni bir sınır tipi yok: `frame()` sayaç metnini üretip **mevcut
  sink'ten** hücre olarak veriyor. `bt-gpu` onu ızgaranın herhangi bir
  harfinden ayırt etmiyor; biçim, renk ve konum kararı `bt-core`'da kalıyor.
- **Saat.** Kare talebinin **üçüncü** sebebi doğuyor: hasar ve hareketin
  yanına **saat**. `bt-core` bir sonraki görünür değişimin ne kadar sonra
  olduğunu sınırdan söylüyor; `bt-gpu::link` o süre dolunca bir kare istiyor.
  Durma koşulu komutun bitmesi — süre `None`'a düşünce saat sönüyor.

## Kararlar

1. **Saat üçüncü kare sebebidir, hareket saatine binmez.** Hareket saati
   link'i ekran hızında uyanık tutar; saniyede bir değişen bir sayı için
   30 saniyelik komutta ~2000 kare üretirdi. Sayaç tiki **içeriktir** (ızgara
   gerçekten değişiyor), yani `icerik` karesi sayması doğru — yasağın
   koruduğu şey "hareketin kendini içerik diye saydırması"ydı, bu onun tersi.
   `bt-gpu::link`'in modül başlığı ve `CLAUDE.md`'nin "boşta sıfır kare"
   maddesi aynı commit'te üçe tamamlanır.
2. **Saatin yeri `bt-gpu`**, `bt-shell` değil. Kare talebi link'in işi,
   `dispatch2` orada zaten var ve sözleşmenin yazılı olduğu modül orası.
   `bt-shell` bu sete hiç girmiyor.
3. **Bir sonraki tiki `bt-core` söyler**, `bt-gpu` hesaplamaz. "Ne zaman
   değişecek" biçimin bir sonucudur (ilk 10 saniyede onda bir, sonra saniye)
   ve biçim `bt-core`'un kararı. `bt-gpu` yalnız süreyi bekler.
4. **Tek `running_since`, blok başına `Instant` değil.** Aynı anda tek komut
   koşar. `BlockLog`'un doc'undaki girdi başına bayt bütçesi 8 → 16 çıkıyor
   (`Option<i32>` + `u32`), 10 000 scrollback'te 80 KB → 160 KB; sayı
   doc'ta güncellenir.
5. **Biçim:** 10 saniyenin altında onda birli (`1.4s`), üstünde tam saniye
   (`12s`), dakikadan sonra `1m 05s`, saatten sonra `1h 02m`. Gerekçe okuma
   sorusu: on saniyenin altında soru "ne kadar sürdü", üstünde "asıldı mı" —
   ikincisinde ondalık gürültü.
6. **Renk `dim`.** Sayaç bloğun **üstverisi**, komutun parçası değil; dock'un
   bağlam satırıyla aynı sınıf ve aynı rolden besleniyor.
7. **Çakışmada sayaç kaybeder.** Komutun mürekkebi sayacın alanına giriyorsa
   sayaç o satırda **hiç çizilmez**. Kullanıcının yazdığı metin hiçbir koşulda
   örtülmez; yön güvenli.
8. **Eşik `1s` bir tasarım sabiti**, ölçüm değil — `docs/OLCUMLER.md`'ye
   girmez. Ayara bağlanması ayrı bir iş (referansta
   `command_duration_threshold` var, `docs/ARASTIRMA.md:98`); bu set ayar
   eklemiyor.

## Kapsam Dışı

- **Ayar anahtarı.** Eşik `1s` sabit; `command_duration_threshold` sonraki iş.
- **Dock'ta süre.** Sayaç yalnız ızgarada, komutun kendi satırında.
- **Çıkış kodunun metni.** Blok şeridinin rengi bugünkü gibi kalıyor.
- **Geçmişe dönük süre.** Set indiğinde açık olan pencerelerin **daha önce**
  koşmuş komutları süresiz kalır; defterde yok, uydurulmaz.

## Göç

Yok. Ayar şeması, tema, terminfo, jeton satırı ve kabuk betiği değişmiyor —
yani `make kur` **zorunlu değil**. Süre tamamen terminalin kendi ölçümü;
sarmalayıcı betiğe tek bayt eklenmiyor.

## Akış

| Phase | İş | Neden bu sırada |
|-------|-----|-----------------|
| phase-1 | Defter + sayaç hücreleri (`bt-core`) | Kendi başına ürün: **biten** komutların süresi görünür olur ve bunun için **hiç yeni kare kaynağı gerekmez** — komut bitince zaten kare var. Riskin tamamı phase-2'ye ertelenmiş olur |
| phase-2 | Saat (`bt-gpu::link`) + sözleşmenin üçe tamamlanması | Canlı tik. Tek mimari risk burada ve phase-1 yeşilken tek başına sınanır |

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ⬜ |
| kapı | ⬜ |

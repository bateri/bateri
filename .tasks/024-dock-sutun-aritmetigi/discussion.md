# Dock sütun saysın — Tartışma

Karar-listesi biçimi. **Yaklaşım çatalı yok** ve bu bir eksiklik değil: kök
tek (indeks sütun sanılıyor), çaresi tek (sütunu genişlikten biriktir). Açık
olan şey **ürün kenarları** — pencere kenarına denk gelen geniş glyph, geniş
hücrede caret, ve grapheme dizilerinin akıbeti.

Aşağıdaki her kararda **boşlukta kullanıcı tarafı seçildi**
(`.claude/skills/rfc` → Bulguyu işleme yolu). 023'ün dersi tam bu: kod kısıtı
gerekçe değil, seçenektir.

## Karar 1: Genişliği kim söyler? → `unicode-width`, ızgaranın crate'i

Sütunu biriktirmek için karakter başına bir genişlik gerekiyor ve `bt-core`
bugün onu hiç okumuyor.

**1A — `unicode-width`'i `bt-core`'un listesine almak.**
- **Artı:** ızgaranın kullandığı **tam olarak aynı** crate ve aynı sürüm
  (alacritty `unicode_width::UnicodeWidthChar` kullanıyor, kilitli 0.2.2).
  Yani dock ile ızgara **tanım gereği** aynı cevabı veriyor.
- **Artı:** grafta zaten var, yani yeni bir bağımlılık **ağacı** doğmuyor —
  `bt-core`'un listesine tek bir kenar ekliyor. `polling`'in 009'da geçtiği
  kapının aynısı (`CLAUDE.md`: "`polling` yeni bir crate değil, `libc` gibi").
- **Eksi:** `CLAUDE.md` bağımlılığı mimari karar sayıyor, yani kayda geçmesi
  gerekiyor — bu dosya o kayıt.

**1B — alacritty'den yeniden ihraç beklemek.** Ölçüldü: alacritty
`UnicodeWidthChar`'ı **içeride** kullanıyor ve ihraç etmiyor. Kol kapalı.

**1C — kendi genişlik tablomuzu yazmak.** İkinci bir genişlik yetkilisi
demek ve ızgaranınkiyle **ayrışacağı gün** belirtisi sessiz: dock bir sütun
kayar. 023'te tam bu riski adıyla yazıp `bt-atlas`'a `unicode-width` sokmayı
reddetmiştim; aynı gerekçe burada **ters yöne** çalışıyor — yetki `bt-core`'da
olmalı ve ızgaranın kaynağıyla aynı olmalı.

→ **1A.** Tek tutarlı cevap ve reddedilmesi ızgara ile dock'un ayrışmasını
kabul etmek olurdu.

## Karar 2: Pencere kenarına denk gelen geniş glyph → asla yarılanmaz

Yatay kaydırma (`skip`) caret sağ kenarı geçince görüntüyü soldan kaydırıyor.
Sütun sayınca yeni bir kenar durumu doğuyor: pencerede **bir** sütun kaldı ve
sıradaki karakter iki sütun istiyor.

- **Yarısını çiz** — sağ yarı kırpılır. **Reddedildi:** 023'ün sözleşmesi
  "kutu ya da tam glyph" ve yarım glyph **sessiz** bir bozulma.
- **Hiç çizme, o sütunu boş bırak** → seçildi. Kullanıcı bir hücre boşluk
  görür ve glyph bir sonraki kaydırmada tam gelir. Aynı kural sol kenarda da:
  `skip` bir geniş karakterin **ortasına** düşerse o karakter atlanır, yarısı
  gösterilmez.

Ölçüt kullanıcı tarafı: boşluk **görünür** bir eksiklik, yarım glyph sessiz
bir bozulma.

## Karar 3: Geniş hücrede caret → ızgarayla parite, tek hücre

Izgarada blok caret bir CJK karakterinin **sol** hücresini kaplıyor (023 bunu
değiştirmedi; `caret_rect` tek hücre veriyor). Dock'ta caret'i iki hücreye
yaymak dock'u ızgaradan **farklı** yapardı ve aynı karakterin iki yüzeyde iki
görüntüsü tam da bu setin kapatmaya geldiği şey.

→ Tek hücre, ızgarayla aynı. İkisini birden genişletmek ayrı bir iş ve
`caret_rect`'in tek yerinden geçer (`bt_gpu::frame`), yani ucuz — ama dock'a
özel yapmak yanlış.

## Karar 4: Tazelik kapısının iki tarafı aynı birimi okusun

Aynanın `last_ink`'i son boşluk olmayan `char`'ı alıyor, ızgara tarafı
hücrenin `c`'sini. Birleştirici kod noktaları (`VS16`, ZWJ, ten rengi)
hücreye **girmiyor** (alacritty `CellExtra`), yani ayna tarafı ızgaranın hiç
görmediği bir karakteri söylüyor ve kapı kalıcı olarak "bayat" diyor.

→ Ayna tarafı **sıfır genişlikli** kod noktalarını atlıyor. Ölçüt yine
`unicode-width` (genişlik 0), yani Karar 1'in ikinci tüketicisi — ve bu iki
tüketicinin aynı kaynaktan beslenmesi şart, yoksa kapı ile çizim ayrışır.

Kapının yanlış yönü **korunuyor**: şüpheli hâl bastırmayı bırakıyor, yani en
kötü ihtimalle satır iki yerde görünür.

## Karar 5: Grapheme dizileri → kapsam dışı, ama atlama düzeliyor

`❤️` (U+2764 + VS16) ve ZWJ aileleri **bu sette de** taban karakteriyle
çiziliyor: atlasın anahtarı `Sprite::Char(char)` ve bir diziyi ifade edemiyor
(023 Karar 1, kapsam dışı).

Ayrım net olsun diye:

| girdi | bu setten sonra |
|---|---|
| `🎉 📁 🚀` (tek kod noktası, 2 sütun) | dock'ta **renkli, iki hücre** |
| `漢字 ｆｕｌｌ` (CJK, fullwidth) | dock'ta **iki hücre**, caret doğru sütunda |
| `❤️` (VS16) | satır **fırlamıyor**; `❤` tek hücre, renksiz (Menlo'nun kendi glyph'i — ölçüldü) |
| `👨‍👩‍👧` (ZWJ) | satır fırlamıyor; taban `👨` çiziliyor, kalanı görünmüyor |

Son iki satır ızgarada **bugün de** öyle görünüyor, yani set iki yüzeyi
eşitliyor — kusuru kapatmıyor. Kapatan şey grapheme seti ve onun ön koşulu
atlas anahtarının `&str` olması.

## Karar Noktaları

Kullanıcıya gidecek **tek** soru kalıyor; ötekiler yukarıda kullanıcı tarafına
kapatıldı:

1. **`region_highlight`'ın sütuna çevrilmesi kapsama girsin mi?** Aralıklar
   karakter indeksinde ve `style_at` öyle kalıyor (doğru olan bu). Ama
   **vurgunun kendisi** artık iki hücre boyunda bir zemin isteyebilir: `zsh`
   bir CJK karakterini vurguladığında bugün tek hücre boyanır. Öneri:
   **kapsama girsin** — baş hücrenin `wide`'ı zemini de iki hücreye yayıyor
   (`bt-gpu` arka planı hücre başına çiziyor, yani spacer sütununa da bir
   arka plan hücresi gerekiyor) ve bu, ızgaranın `WIDE_CHAR_SPACER`'a zemin
   vermesinin aynısı. Bedeli bir hücre daha, kazancı yarım boyanmış bir
   vurgunun olmaması.

## Muhakeme

**Panel koşmadı.** `/rfc` adım 6'nın koşulu iki şartın **birlikte**
sağlanması: `discussion.md`'de birden çok yaklaşım **ve** pahalı sınıfa
dokunan bir seçim. İkincisi var (yeni crate bağımlılığı) ama **birincisi
yok** — Karar 1'in üç kolundan ikisi ölçümle kapandı (1B: ihraç yok; 1C:
ikinci genişlik yetkilisi) ve kalan tek kol. Panelin sınayacağı bir çatal
olmadığı için üç `opus` ajanı açmak gürültü olurdu.

**Kapalı kolun kaydı yine burada:** 1C reddedildi ve gerekçesi 023'ün kendi
kuralının tersine çevrilmiş hâli — orada `bt-atlas`'a genişlik sokmayı
reddetmiştim, burada `bt-core`'a sokmak **zorunlu**, çünkü yetkinin yeri
ızgarayla aynı katman.

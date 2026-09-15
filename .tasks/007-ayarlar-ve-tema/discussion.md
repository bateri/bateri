# Ayarlar, tema ve font — Tartışma

Birbirinden bağımsız dokuz karar. Bağlam `context.md`'de, burada
tekrarlanmıyor. Öneriler **iki panel turundan sonraki** hâlidir.

**Kullanıcı yönü (2026-09-15):** tema uygulama **açıkken** değişir — bu bir
seçenek değil, ürünün kendisi. Geri kalan kararlar **arayüz ve kullanıcı
deneyimi için iyi olan** seçenekle verilir. İlk turun "açılışta bir kez oku"
önerisi bu yönle düştü (`## Yön değişikliği`).

## Karar 0: Kapsam

**Girer:**

1. `~/.config/bateri/settings.toml` ve `themes/` — okunur, **canlı izlenir**.
2. `scrollback` — ayar modelinin ilk tüketicisi.
3. Tema: biçim, iki gömülü tema (koyu `bateri`, açık `bateri-light`),
   kullanıcı tema dizini, sistemin açık/koyu görünümünü takip.
4. Font: `family`, `size`; canlı değişir; Cmd +/−/0 ile geçici punto.
5. OSC 52 yazma yönü.
6. Ölçeğin iki kapısı borcunun kararı.
7. **Ana menü:** uygulama menüsü (About, Ayarlar…, Çıkış), Düzen (Kopyala,
   Yapıştır), Görünüm (Tema ▸, punto).
8. **Hata görünür:** bozuk ayar pencerede söylenir.
9. `docs/AYARLAR.md` — ilk anahtarla doğar, her phase'de büyür.

**Girmez:**

- **Ayar penceresi.** "Ayarlar…" dosyayı varsayılan editörde açar.
- **`line_height`** — `Metrics` geometrisi (taban çizgisi, kural zarfı,
  descender güvencesi) kendi sınamalarını ister.
- **Gömülü font** (JetBrains Mono) — paketleme ve OFL atfı.
- **Dört durum rolü** — tüketicisi 013.
- **Geniş tema kataloğu** — 009 materyalleriyle.
- **Tema değişiminin çalışan uygulamaya bildirilmesi** (DEC 2031): alacritty'de
  yok; açık nvim temanın değiştiğini kendiliğinden görmez.
- **Kapatma onayı** (`confirm_close`): Cmd-Q bu setle çalışmaya başlıyor ve
  açık vim'i sormadan kapatır — teslim notuna yazılır.
- **LS_COLORS / prompt renkleri** — 012. **OSC 52 okuma yönü** — Karar 5.
- **"Önce ata, sonra sor" renk sorgusu sınırı** (`session.rs:473-478`) —
  kökü alacritty'nin kilit altındaki `Colors` tablosu; yorum yönlenir.

## Karar 1: Ayar modeli nerede ve nasıl okunur?

**Öneri:** Ayrıştırma **ve biçim koruyan yazma** `bt-core`'da saf
fonksiyonlar: metin → `Settings` + tanılar; metin + (anahtar, değer) → yeni
metin. `child.rs`'in "saf karar + ince sistem sarmalayıcısı" örüntüsü;
`toml_edit` yalnız `bt-core`'a girer. Dosya okuma, izleme, yazma
`bt-shell`'de; kök dizin parametre, üretimde `$HOME/.config/bateri/`.

**Kapılar hermetik — tek dal.** Süreli koşuda (`BT_RUN_SECONDS`) dört giriş
kapalı: dosya okuma, izleme, sistem görünümü takibi, Tema menüsünün
`themes/`'ten dolması. Gömülü koyu tema ve varsayılan puntoyla koşar.
Kapılık kodda `app.rs:803`'teki `run.map` gibi **tek bir dal**, dört ayrı
koşul değil. Sınamalar gerçek `HOME`'a dokunmaz. Bedeli: dosyadan ekrana
giden kabloyu hiçbir kapı görmez → geçici dizinde sınama + göz kontrolü.

**Hata politikası — tek kural:**

- Dosya **ayrıştırılamıyorsa**: canlı yenilemede **hiçbir şey uygulanmaz**,
  ekran olduğu gibi kalır; açılışta varsayılanlar (OSC 52 hariç, Karar 5).
- Dosya **ayrıştırılıyorsa**: her anahtar ya kendi geçerli değerini ya
  varsayılanını alır — açılışta da canlıda da aynı. Yanlış tip tanı üretir.
- Bilinmeyen anahtar yoksayılır, tanı yok (008'in `[motion]`'ı düşmemeli).

Ayrıştırıcı önceki `Settings`'i bilmez, saf kalır. Anlam aralığı da bilmez:
punto için yalnız sonlu/pozitif. `scrollback` tavanı bir **kaynağa** bağlanır.

**Anahtar adları:** `[appearance] theme / light_theme / dark_theme`,
`[font] family / size`, `[clipboard] osc52`, `[terminal] scrollback`.
ARASTIRMA'nın `font_size`, `family`'sinden bilerek sapma: bölümlü ad 008'in
`[motion]`'ıyla tutarlı.

**Bağımlılık: `toml_edit`** — CLAUDE.md'deki `toml` + `serde` yerine. Menüden
seçilen tema dosyaya yazılır ve sözleşme "tanımadığını bırakır" diyor; `toml`
+ `serde` yorumları, düzeni ve bilinmeyen anahtarların yerini kaybeder.
`toml_edit` okuma ile biçim koruyan yazmayı tek crate'te verir; serde
gerekmiyor. `Cargo.lock`'a birkaç geçişli crate girer, phase riskli;
CLAUDE.md'nin taban satırı aynı commit'te düzelir.

## Karar 2: Ayarlar ne zaman okunur? → canlı (kullanıcı kararı)

`dispatch2 0.3.1`'in vnode kaynakları; yeni crate yok, değişiklik gelmedikçe
hiçbir şey uyanmaz.

- **Hem dizin hem dosya izlenir.** Dizin kaynağı (`WRITE`) yalnız dosya
  doğunca/silinince/yeniden adlandırılınca haber verir; **yerinde yazma**
  (`echo >>`, nano) ve **symlink hedefindeki** kayıt dizine iz bırakmaz. Bu
  yüzden `settings.toml` ve **etkin kullanıcı teması** dosyaları ayrıca
  `WRITE | EXTEND | DELETE | RENAME` ile izlenir (açılış symlink'i izler,
  kaynak hedefe bağlanır).
- **Her olayda hepsi yeniden kurulur:** bütün kaynaklar iptal, dosya yeniden
  okunur, yeni `Settings` öncekiyle karşılaştırılır, kaynaklar yeniden
  kurulur. Birleştirme ayrı mekanizma istemez (fark yoksa uygulanacak şey
  yok); taşınan/yeniden yaratılan dizinin bayat tanıtıcısı da böyle düşer.
- **Dizin yoksa kaynak kurulmaz.** "Ayarlar…" ve menüden tema seçimi dizini
  ve dosyayı oluşturup kaynakları kurar. Dosyayı ilk kez kabuktan elle
  oluşturan kullanıcı için `AYARLAR.md` bir satır yazar.
- `themes/` dizini liste için izlenmez: Tema menüsü açılırken dizini okur
  (`menuNeedsUpdate:`).
- **Uygulama zinciri** (ana thread), yalnız değişen: tema → takas + kare
  isteği; font → atlas yeniden kurulumu → `refresh_geometry`; `osc52` /
  `scrollback` → `Term::set_options` + kare isteği.
- **`Term::set_options` bütün `Config`'i değiştirir** (alacritty
  `term/mod.rs:499-516`): kurucudaki `..Config::default()` örüntüsüyle
  çağrılsaydı `osc52` değişimi geçmişi 10 000'e **geri dönülmez** kırpar,
  `scrollback` değişimi OSC 52'yi açardı. `Config` her seferinde `Settings`'in
  **tamamından** kurulur; `Session` güncel değerleri tutar.
- **Kare isteği bizim bayrağımız:** kod alacritty'nin hasarını bilerek
  okumuyor (`session.rs:413-416`); tema takası ve `set_options` sonrasında
  `request_frame()` çağrılır (geçmiş küçülünce kaydırma ofseti de değişir).
- Olay işleyicisi fonksiyon işaretçili varyantla kurulur; `bt-shell`'e
  `block2` kenarı gerekmez.

## Karar 3: Tema biçimi

Sekiz rollü model tasarımda kalır; 007'de tüketicisi olan dört rol:
`background`, `foreground`, `dim` (sönük ön plan), `accent` (imleç). Dört
durum rolü 013 ile açılır — bilinmeyen anahtar yoksayıldığı için geriye
uyumlu.

- **A) Tam tablo, her anahtar opsiyonel,** eksik anahtar gömülü `bateri`'den.
- **B) Yalnız roller, 16 ANSI türetilir** — zevk ve renk uzayı kararı.
- **C) Roller + opsiyonel ANSI, eksikler türetilir** — B'nin sorusunu taşır.

**Öneri: A.** Kullanıcının gerçek iki yolu "gömülü temayı seç" ve
"internetten bir paleti yapıştır"; ikisi de tam tablo. `AYARLAR.md` "bir
gömülü temayı kopyalayıp değiştir" yolunu gösterir.

**İki gömülü tema, sistem görünümü varsayılan:**

- `bateri` — bugünkü `color.rs` paleti; `bateri-light` — yeni, göz
  kontrolüyle kabul.
- `theme = "system"` (varsayılan) → `light_theme` / `dark_theme` (varsayılan
  `bateri-light` / `bateri`); `theme = "{ad}"` → sabit. **Ayrı anahtarlar**
  çünkü menüden sabit bir tema seçmek yalnız `theme`'i yazar, kullanıcının
  açık/koyu çifti yerinde kalır; "Sistemle değiş"e dönünce çifti geri gelir.
- Görünüm değişimi `BateriView`'ın `viewDidChangeEffectiveAppearance`'ından
  gelir; view uygulayıcıya hedefsiz eylemle (responder zinciri → app
  delegate) ulaşır. Hermetik dal uygulayıcıda.

**Sönük renk zemine göre.** Açık temada `×2/3` sönük metni koyulaştırır. SGR
2'li renk zemine doğru karıştırılır (sRGB 8-bit, vte'nin çarpımıyla aynı
uzay); varsayılan ön plan + SGR 2 `dim` rolünü alır — çözümden **önce** açık
dal, **ters videolu dal** (`session.rs:890-893`) dahil. Koyu temanın sönük
değerleri kayar: bilinçli değişiklik. **Sıra:** önce çizim yolundan geçen,
**sabit değerli** bir sönük renk bekçisi (bugünkü bekçiler `color::dim`'i
kendisi hesaplıyor ya da çizim yolunda okunmayan bir girdiye bakıyor —
`session.rs:2088`, `color.rs:331`), kural değişimi **ayrı commit**'te ve
bekçinin değerleri bilerek güncellenir.

**Palet takası ve tek kaynak.**

- Tema `Adapter`'ın paylaşılan gövdesinde, `size`'ın yanında **yaprak kilit**
  altında (`session.rs:481`'deki `size` emsali): kilit tutulurken başka kilit
  alınmaz. `frame()` temanın paylaşılan kopyasını `Term` kilidinden **önce**
  alır; renk sorgusu `Term` kilidi altında kısa bir okuma yapar. Takas: yaz,
  `request_frame()`.
- `frame()` imzası değişmez; zemin atlaması (`session.rs:895,999`), clear ve
  imleç rengi (`link.rs:358,371`), renk sorgusu **aynı** temadan.
- `Theme` `pub` tipinde alacritty tipi yok; `Theme::BATERI` `const`
  (bt-gpu sınamaları `const` bağlamda kullanıyor, `frame.rs:323`).
  `DEFAULT_BG`/`DEFAULT_CURSOR` kalkar; üretim sabit okumaz.
- **Palet bekçisi taşımadan önce:** bugünkü 19 değer (16 ANSI, zemin, ön
  plan, imleç) sabitler silinmeden aynı commit'te sabit listeye bağlanır.

Tema adı çözümü: önce `themes/{ad}.toml`, sonra gömülü; bulunamazsa önceki
tema (açılışta `bateri`) + görünür hata.

## Karar 4: Font

- `[font] family` ve `size`; **canlı değişir.** `Renderer` aile ve puntoyu
  tutar; aile atlas anahtarına girer. Değişim atlası yeniden kurar,
  `refresh_geometry` grid'i ve PTY'yi yeniden boyutlar. Geçmiş yeniden sarılır
  ve bu `Term` kilidi altında ana thread'de olur → **ölçüm bekliyor**.
- Punto kırpması `bt-atlas`'ta **sessiz** kalır: `punto × ölçek`'e bağlı
  olduğu için tanısı ekran değiştikçe gelip giderdi. Dolan atlas tofu çizer —
  görünür kayıp, tasarım gereği (`bt-atlas/src/lib.rs:39-42`).
- **Aile bulunamazsa** zincire (SF Mono → Menlo) düşülür ve bu **görünür**:
  `bt-gpu` kendi tipinde bir font bildirimi yayımlar (`bt-atlas` tipi
  sızmaz, 003 R5). Eşaralıklı olmayan aile reddedilmez, uyarısı aynı yoldan.
- **Cmd + / Cmd − / Cmd 0:** geçici büyüt/küçült/ayara dön, dosyaya
  yazılmaz. Dosyadaki `size` değişince geçici punto sıfırlanır — iki punto
  kaynağı yarışmaz.

## Karar 5: OSC 52

- `clipboard.osc52`: `"off"` / `"copy"`, varsayılan `"copy"`; canlı değişir
  (`Config` tamamından kurulur, Karar 2).
- **Kapalıya düşer:** tanınmayan değer (`"paste"`, `false`) → `"off"`;
  açılışta ayrıştırılamayan dosya → `"off"`. Canlı yenilemede ayrıştırılamayan
  dosya hiçbir şey uygulamaz, kullanıcının son açık seçimi kalır.
- Okuma yönü yok (006 Karar 5).
- Köprü: `Adapter` → `Wake`'e yeni çağrı (okuyucu thread, `Term` kilidi
  tutulurken) → `ShellWake` ana kuyruğa → `clipboard::copy`. **Son yazma
  kazanır, kilitsiz:** tek yuva atomik takasla dolar; yuva boşken dolduran
  çağrı tek iş atar, iş yuvayı boşaltıp yazar (`wake.rs`: uygulayan kilit
  almaz). `TestWake` (`session.rs:1739`) da uygular.
- `p`/`s` hedefleri macOS'ta yok → yoksayılır. Sınama dışarıdan verilen
  panoya yazar.

## Karar 6: Ölçeğin iki kapısı

**Öneri: birleştirme, borcu gerekçesiyle kapat.** Tek çağıran
(`sync_geometry`, `app.rs:1035-1045`) ölçeği bir kez okuyup iki çağrıya aynı
yerelden veriyor; font ayarı ölçeğe değil aile ve puntoya dokunuyor.
Kapanış `sync_geometry`'nin doc'una tek cümle.

## Karar 7: Tema nereden seçilir?

- **Görünüm ▸ Tema ▸** — "Sistemle değiş", sonra gömülü ve kullanıcı temaları,
  seçili olan işaretli. Seçim **yalnız dosyaya yazar**; uygulamayı izleyici
  yapar (tek uygulama zinciri).
- **Yazma:** dosya o anda taze okunur, `bt-core`'un biçim koruyan
  fonksiyonundan geçer, **yerinde** yazılır — symlink'li (dotfiles) dosyada
  hedefe yazar, bağ kopmaz. Dosya ayrıştırılamıyorsa **yazılmaz**, hata
  görünür (içeriği ezmek yerine). Yazma hatası da görünür.
- **Ana menü bu setle doğar:**
  - Uygulama menüsü: About (Credits paneli — 006'dan kalan borç kapanır),
    "Ayarlar…" (Cmd ,) — dosya yoksa yorumlu şablonla oluşturur, varsayılan
    editörde açar, Çıkış (Cmd Q).
  - Düzen: Kopyala/Yapıştır `BateriView`'ın `copy:`/`paste:` seçicilerine
    taşınır; `command_shortcut`, `run_shortcut` ve `Shortcut` silinir.
    **Command'lı tuşu yutan `return` kalır** (`view.rs:319`): menüde
    karşılığı olmayan Cmd-T kabuğa "t" yazmamalı. Menü ile köprünün silinmesi
    **aynı commit**; `view.rs:491-493` ve `app.rs:332` yorumları düzelir.
  - Görünüm: Tema ▸, Büyüt/Küçült/Gerçek boyut.
- Komut paleti (⌘⇧P) girmez: overlay çizimi ister.

## Karar 8: Hata nasıl görünür?

- **Pencere alt başlığı** (`NSWindow.subtitle`). Modal pencere yok: canlı
  düzenlemede her kayıtta açılan uyarı kullanıcıyı durdururdu.
- **Kaynak başına yuva:** ayar dosyası, tema dosyası, font, yazma. Bir yuva
  yalnız **kendi** kaynağı düzelince boşalır; alakasız başarılı bir okuma
  başka yuvayı silmez. Alt başlığın tek sahibi bir fonksiyon; ayar
  uygulayıcısının ve `sync_geometry`'nin sonunda çağrılır. Birden çok dolu
  yuvada ilki ve sayısı.
- stderr'e de basılır (`bateri:` öneki).
- **Görünürlük doğrulanmadı:** pencere araç çubuksuz düz `Titled`
  (`app.rs:253-256`) ve tam ekranda başlık gizlenir. İlgili phase'in **ilk
  adımı** alt başlığın bu kurulumda çizildiğinin göz kontrolü; çizilmiyorsa
  phase durur, yol kullanıcıya gelir.

## Karar Noktaları

→ ✅ Tek açık soru (`toml_edit` bağımlılığı, kullanıcının JSON sorusuyla
birlikte) kapandı; kayıt `## Karar`'da.

## Muhakeme (2026-09-15)

1. tur "açılışta bir kez oku" önerisi üstünde koştu.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Kabul edilen itirazlar → öneri değişikliği:**

- `dim` rolünü paletin 268 girdisine bağlamak ekranda hiçbir şey değiştirmez
  (`session.rs:934-937`) — Codebase-fit → çözümden önce açık dal.
- "Zemin ve imleç kareyle, aynı kilit altında" `frame()` imzasını elletirdi —
  Sadelik, Codebase-fit → tema oturumdan okunur, imza sabit.
- `DEFAULT_BG`'nin kalkması sınamaları elletirdi — Sadelik → `Theme::BATERI`
  `const`.
- Tüketicisiz dört durum rolü adları donduruyor — Sadelik, İşletme → 013.
- Punto aralığının iki sahibi olurdu, ölçütü `punto × ölçek` — Codebase-fit
  → kırpma atlasta.
- Kapılar kullanıcının ayar dosyasına bağlanırdı — İşletme → süreli koşu
  dosyayı açmaz.
- OSC 52 bozuk dosyada açığa düşerdi — İşletme → `"off"`.
- Durmadan OSC 52 basan uygulama ana kuyruğu doldurur — Codebase-fit → son
  yazma kazanır.
- Palet taşınırken yazım hatasını gören sınama yok — İşletme → 19 değer
  sabit listeye.
- `docs/AYARLAR.md` son kalemdi — İşletme → ilk anahtarla doğar.
- `scrollback` tavansız; anahtar adları referanstan sapıyor — İşletme →
  tavan kaynağa, sapma kayıtlı.
- Karar 6'nın "`Surface` ile `Renderer`'ı bağlar" gerekçesi zayıftı
  (`link.rs:413-419`) — Codebase-fit → gerekçeden çıktı.
- `session.rs:473-478`'in sınırı temayla çözülmüyor — Codebase-fit → kapsam
  dışı.
- *(Yön değişikliğiyle aşılanlar: sönük rengin `×2/3` kalması, temanın
  değişmez olması, ailenin kurucu parametresi olması — tablo aşağıda.)*

**Reddedilenler:**

- *"`dim` rolü yalnız paletin 268 girdisini besler"* (Sadelik) — girdi çizim
  yolunda okunmuyor, rol etkisiz kalırdı.

## Yön değişikliği (2026-09-15, kullanıcı)

Kullanıcı "tema tabii ki uygulama açıkken değişecek; geri kalanı UX için iyi
olanla seç" dedi. 1. turun şu sonuçları aşıldı:

| 1. tur sonucu | yeni hâl | sebep |
|---|---|---|
| açılışta bir kez oku | dizin + dosya izleme | kullanıcı kararı |
| tema değişmez, kilitsiz okunur | takas edilebilir, yaprak kilit | canlı değişim |
| aile kurucu parametresi | atlas anahtarında | aile artık değişiyor |
| sönük renk `×2/3` kalır | zemine doğru karıştırma | açık tema geldi |
| ayar dosyasına yazma yok | menüden tema seçimi yazar, `toml_edit` | seçim yeniden açılışta kalmalı |
| gömülü tek tema | koyu + açık, sistem görünümü varsayılan | açık/koyu deneyimi |
| tanı yalnız stderr | pencere alt başlığı | hata görünmeli |

## Muhakeme — 2. tur (2026-09-15)

Yön değişikliğinden sonraki hâl üstünde koştu; canlı değişim tartışmaya
açılmadı.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yönü ve katman düzenini onayladı; `toml_edit`, alt başlık, menü
kapsamı ve Karar 6 temiz bulundu. Sorunlar eklem yerlerinde.

**Kabul edilen itirazlar → öneri değişikliği:**

- Yalnız dizini izlemek yerinde yazmayı, symlink hedefini ve yeniden
  yaratılan dizini kaçırır — üçü → dosya kaynakları eklendi, her olayda
  hepsi yeniden kurulur.
- "En yakın üst dizini izle" durum makinesi, "Ayarlar…"ın zaten kapattığı bir
  durum için — Sadelik → dizin yoksa kaynak yok.
- `themes/`'i liste için izlemek gereksiz — Sadelik → `menuNeedsUpdate:`.
- "Son iyi değer" tablosu ayrıştırıcıya önceki `Settings`'i sokuyor, UX
  kazancı yok — Sadelik → tek kural: ayrıştırılamazsa hiçbir şey uygulanmaz,
  ayrıştırılırsa anahtar başına değer ya da varsayılan.
- `Term::set_options` bütün `Config`'i değiştiriyor; kurucu örüntüsüyle bir
  anahtar ötekini sıfırlar, geçmiş geri dönülmez kırpılır (alacritty
  `term/mod.rs:499-516`, `grid/mod.rs:154-158`) — İşletme, Codebase-fit →
  `Config` her seferinde `Settings`'in tamamından.
- "Tam hasar" alacritty'nin bayrağıydı, kod onu okumuyor
  (`session.rs:413-416`) — Codebase-fit → `request_frame()`.
- Menü hem uyguluyor hem yazıyordu — Sadelik → yalnız yazar, izleyici uygular.
- Geçici dosya + yeniden adlandırma symlink'i düz dosyaya çevirir; korunduğu
  yarış yok (yazma ve okuma aynı ana kuyrukta) — Sadelik, İşletme → yerinde
  yazma.
- Ayrıştırılamayan dosyaya menüden yazmak içeriği ezer — İşletme → yazılmaz,
  hata görünür.
- Menüden sabit tema seçmek açık/koyu çiftini kaybettirir — İşletme →
  `light_theme` / `dark_theme` ayrı anahtar.
- Tek alt başlık yuvası alakasız bir okumada silinir; font tanısının alt
  başlığa yolu yok — İşletme, Codebase-fit → kaynak başına yuva, tek sahip,
  `bt-gpu`'nun kendi tipinde font bildirimi.
- Kırpılan punto tanısı `punto × ölçek`'e bağlı, ekran değiştikçe gelip
  giderdi — Sadelik, Codebase-fit → kırpma sessiz.
- Cmd +/−/0 ikinci bir punto kaynağı — Sadelik → dosyadaki `size` değişince
  geçici punto sıfırlanır.
- `view.rs:309-319`'u tümden silmek Cmd-harfleri kabuğa yazdırır —
  Codebase-fit, İşletme → yutan `return` kalır, menü ile köprü aynı commit.
- OSC 52 yuvası `Mutex` olursa `wake.rs`'in "kilit almaz" kuralını çiğner —
  Codebase-fit → atomik takas.
- Tema kilidi için "sözleşme" fazla — Sadelik, Codebase-fit → yaprak kilit,
  `size` emsali, `frame()` kopyayı `Term` kilidinden önce alır.
- Sönük renk bekçileri kuralı görmüyor (`session.rs:2088` beklenen değeri
  kendisi hesaplıyor); ters videolu dal unutulmuş — İşletme → sabit değerli
  çizim yolu bekçisi önce, kural ayrı commit, ters video dahil.
- Hermetik kapı dört giriş noktasından birini (menünün `themes/`'ten
  dolması) saymıyordu; dört ayrı koşul drift eder — İşletme → tek dal.
- Görünüm değişiminin view'dan uygulayıcıya yolu yok (`view.rs:130-156`) —
  Codebase-fit → hedefsiz eylem, responder zinciri.
- Alt başlığın düz `Titled` pencerede çizildiği doğrulanmadı — Codebase-fit →
  phase'in ilk adımı göz kontrolü.
- Cmd-Q artık açık vim'i sormadan kapatır — İşletme → teslim notu, kapatma
  onayı kapsam dışı.

**Plana devredilen notlar:**

- Önerilen phase sırası (İşletme): ayar modeli + `toml_edit` + tanı yuvaları
  + hermetik dal → palet bekçileri + tema takası → sönük kural (ayrı commit) +
  `bateri-light` + görünüm takibi → izleme + fark + `Config`'in tamamından
  kurulması → canlı font + Cmd +/−/0 → menü + Cmd-C/V taşıması + tema yazma →
  OSC 52 köprüsü.
- Riskli phase'ler: `toml_edit` (`Cargo.lock`); tema takası ve OSC 52
  (okuyucu thread yolu → `make test-yaris`).
- İzleme ve yazma senaryoları (yerinde yazma, symlink, yeniden yaratılan
  dizin, ayrıştırılamayan dosyaya yazma) geçici dizinde sınanır.
- Ölçüm bekliyor: `frame()`'e giren tema kilidinin kare süresine etkisi;
  canlı font değişiminde geçmişin yeniden sarılması.
- Göz kontrolü: tema menüsü, açık tema, Cmd-C/V/Q, vim açıkken font değişimi,
  alt başlık.
- `color.rs:52-53`, `:172-175` yorumları düzelir; `renderer.rs:1206-1210`'daki
  "clear rengi ara ton" özelliği korunur.

**Reddedilenler:**

- *"Atlas doldu" alt başlığa girsin* (İşletme) — dolan atlas tofu çiziyor,
  kayıp zaten ekranda görünür (`bt-atlas/src/lib.rs:39-42`); alt başlık aynı
  bilgiyi ekran değiştikçe açıp kapatırdı.
- *Yazmadan önce içeriği son okunanla karşılaştır* (İşletme) — yazma zaten
  dosyayı o anda taze okuyup aynı çağrıda yazıyor; ayrı karşılaştırma aynı
  pencereyi daraltmıyor.
- *Cmd +/−/0 istenen UX listesinde yok* (Sadelik) — kullanıcının "UX için iyi
  olanı seç" yönünde standart terminal davranışı; ikinci kaynak sorunu
  sıfırlama kuralıyla kapandı.
- *`command_shortcut` saf karar olarak kalsın, `copy:`/`paste:` onu çağırsın*
  (İşletme) — menü kısayolu bildirimsel; köprünün kendi doc'u menü günü
  silinmesini söylüyor (`view.rs:491-493`). Yutma sınaması `keyDown:`'ın
  kalan dalına bağlanır.

## Karar (2026-09-15, kullanıcı onayı)

- **Seçilen:** Karar 0–8'in 2. panel turundan sonraki hâli. Karar 2 (canlı
  değişim) kullanıcının kendi kararı; geri kalanı kullanıcının "arayüz ve
  kullanıcı deneyimi için iyi olanı seç" yönüyle verildi, kullanıcı "önerini
  uygula" ile onayladı.
- **Seçilen: TOML, bağımlılık `toml_edit`** (Karar 1). Kullanıcı JSON'u sordu;
  TOML'da kalındı: "Ayarlar…"ın açtığı şablon yorum satırı ister, dosya elle
  ve canlı düzenleniyor (TOML'da sonda virgül/tırnaksız anahtar gibi tek
  karakterlik bozulma sınıfı yok), referans ürün (`settings.toml`) ve
  alacritty'nin tema ekosistemi TOML, menüden yazma biçimi korumalı.
  `toml_edit` okuma ile biçim koruyan yazmayı tek crate'te veriyor, `serde`
  gerekmiyor. CLAUDE.md'nin "`toml` + `serde`" taban satırı phase-1'de düzelir.
- **Reddedilen: JSON / JSONC** — standart JSON'da yorum yok, JSONC ayrı
  ayrıştırıcı ister; elle düzenlemede sözdizimi hatası sınıfı daha geniş ve
  canlı okumada alt başlığa sık hata düşürür; biçim koruyan yazıcı seçeneği
  dar. Tek artısı olan şema ile otomatik tamamlama TOML'da da var.
- **Reddedilen: açılışta bir kez okuma** — kullanıcı: tema uygulama açıkken
  değişir.
- **Reddedilen: `toml` + `serde`** — menüden yazmada yorumları, düzeni ve
  bilinmeyen anahtarların yerini kaybeder.
- **Reddedilen: tema biçiminde türetim (B, C)** — renk uzayı ve zevk kararı;
  kullanıcının iki gerçek yolu tam tablo.
- **Reddedilen: ölçeğin iki kapısını birleştirmek** — kapattığı bir hata yok.

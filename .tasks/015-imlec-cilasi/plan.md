# İmleç cilası

## Hedef

İmleç hem **doğru anda kıpırdasın** hem **iyi görünsün**: hızlı komutta yarıda
dönen sıçrama kalksın, caret'in köşesi yuvarlansın ve çevresinde hafif bir hale
olsun, odakta olmayan pencerede içi boşalsın.

## Gereksinimler

- **R1 — Sıçrama azalır.** Hızlı komutta (`ls`) caret'in dock → ızgara → dock
  gidiş-dönüşü **görünmez** olur.
  - **R1.1** — Çare **histerezis**, eşik değil: Dock→Grid geçişi N ms tutulur,
    o süre içinde geri dönerse geçiş hiç olmamış sayılır. Devir Enter'daki
    `line-finish`'te başlıyor (`8133;e` → ayna `Idle`), `CommandStart`'ta
    **değil** — `running_since` o anda `None` ve ona bağlı bir eşik üçüncü bir
    devir doğururdu.
  - **R1.2** — Damganın yeri `ShellLog`; geçiş `apply_scan`'de gözlenir (tek
    giriş noktası, yaprak kilidin altında). `caret_home` üçüncü bir argüman
    alır.
  - **R1.3** — Eşiğin kalan süresi `Cursor::next_tick` ile istenir ve
    `caret_home` ile **aynı kilit turunda** hesaplanır; `resolve_blocks`'unki
    ile `min`'lenir — bugün o yol değeri **eziyor** ve koşan bloğun çıpasının
    görünür olmasına bağlı.
  - **R1.4** — İddia **azaltma**, kaldırma değil: animasyon ~230 ms'de
    yerleşiyor, altındaki her eşik belirtiyi küçültür ama bitirmez. Phase
    `## Yayın Etkisi`'ne "ölçüm bekliyor" yazar.
- **R2 — Caret'in kendi fragment'i olur.** `cell_bg.metal`'de kardeş bir
  fragment; `cell_bg_vertex` ve `Instance` **aynen** kullanılır.
  - **R2.1** — Yeni `#[repr(C)] ↔ .metal` çifti **doğmaz**; şekil
    parametreleri uniform'dan gelir. İç dikdörtgen olarak var olan
    `CursorBlock` okunur.
  - **R2.2** — `cell.metal` **hiç açılmaz**; `CursorBlock` 32 baytta kalır.
  - **R2.3** — Shader **her alanı okur**: okunmayan bir uniform alanı hiçbir
    pikselin doğrulamadığı bir sözleşme olurdu.
- **R3 — Şekil sayıları türetilir.** Kenar kalınlığı `CellMetrics::rule_px`,
  hale payı `gutter_px`, yarıçap hücre ölçüsünün oranı. Türetilemeyen kalırsa
  `const` doc'unda "seçilmiş, ölçülmemiş" **ve hangi metriğin neden yetmediği**.
- **R4 — Yuva seçimi şişmemiş dikdörtgene bakar.** Hale payı ayak izini
  büyütüp caret'i dock yuvasına kaydırırsa caret ızgaranın glyph'lerinden
  **sonra** çizilir ve altındaki harfi boyar.
- **R5 — `caret_rect` iki dikdörtgen döndürür:** boyanan ve **opak iç**.
  Değişmez ("tek yer, iki tüketici") kırılmıyor, eksik tanımlıydı — içi boş
  imleçte boyanan var, opak iç yok.
- **R6 — Hale blink alfasıyla söner** ve bekçisi dikdörtgenin **dışını**
  örnekler. Mevcut `cursor_alpha_is_blended_on_the_gpu` yalnız caret'in kendi
  hücresine bakıyor, yani belirtiyi göremez.
- **R7 — Odak `bt-core`'a girmez.** `DisplayLink::set_focused(bool)`;
  `AppDelegate` zaten `NSWindowDelegate`.
  - **R7.1** — Hermetik koşuda odak **hiç okunmaz** (`resolve_reduce_motion`
    emsali): kapı bir makinede yeşil bir makinede kırmızı düşerdi. Kapının
    yeri **çağrı**, varsayılan değil — `focused: true` varsayılanı yetmez,
    çünkü `make duman` koşarken açılan bir Spotlight `windowDidResignKey:`
    doğurur ve o da bir kare ister.
  - **R7.2** — `set_focused` **aynı değerde no-op** (`Session::set_theme`
    emsali); yoksa açılıştaki key olayı bedava bir içerik karesi yazar.
  - **R7.3** — İçi boş imleç `CaretShape`'e **eklenmez**: o enum ayar
    dosyasının sözlüğü, odak ise şekle dik bir eksen.
  - **R7.4** — **Odaksız pencerede blink durur ve imleç görünür kalır**
    (kullanıcı kararı, 2026-09-19). Dikkat çekmek için var olan bir hareketin
    kimsenin bakmadığı pencerede koşması boşa yanan pildir. Kabul edilen bedel
    adıyla: `bt-gpu`, `Cursor::blink`'in ("buradan geçen şey yalnız 'sönsün
    mü'") **ikinci sahibi** olur. Mekanizma yeni değil — `content_frame`'in
    kapısına üçüncü bir terim giriyor ve "kapalı blink görünür kalır"
    değişmezi (bugün gizli imleci koruyan) üçüncü tüketicisini kazanıyor.
- **R8 — Ayar anahtarı yok.** Geri alma yolu phase commit'ini revert etmek ve
  bunu gerçek kılmak için `yarıçap = 0` / `hale alfası = 0` **desteklenen ve
  sınanan** bir yoldur.
- **R9 — Kırpma sınırları yazılı.** Beşi: dock zemininin **örtmesi**,
  `origin_y`'de **kırpılma**, viewport'ta **kırpılma**, artık şeritte
  **kalma**, ve komşu arka planların **solması**.
- **R10 — Belgeler kodla aynı commit'te.** `CLAUDE.md` (caret cümleleri,
  `bt-gpu` satırının pipeline sayısı), `crates/bt-gpu/src/lib.rs` başlığı,
  `docs/YOL-HARITASI.md` (**altıncı kayma** + kendi satırı), üç bayat doc
  (`Frame::move_caret`, `Frame::push`'un `debug_assert`'i, `shell::caret_home`'un
  serbest fonksiyon gerekçesi) ve odak phase'inde `session.rs`'in "odak bugün
  sınırdan geçmiyor" cümlesi.

## Yaklaşım

**Önce zamanlama, sonra yüzey, en son odak.**

1. **Histerezis** (`bt-core`): shader'a hiç dokunmuyor, kullanıcıyı günlük
   rahatsız eden o ve tek başına sevk edilebilir.
2. **Kardeş fragment** (`bt-gpu`): pipeline takası **dejenere değerlerle**
   (yarıçap 0, pad 0, hale alfası 0) inebilir — çıktı bit bit bugünküyle aynı
   olur ve mevcut piksel sınamaları paritenin kanıtı olur. Şart: o kolda kenar
   **sert adım** kalmalı, `smoothstep` daha girmemeli.
3. **Odak + içi boş imleç**: setin tek crate'ler arası sinyali. Set uzarsa
   doğal kesme çizgisi burası.

## Kararlar

1. **Sıçramanın çaresi histerezis** (R1). Gerekçe ve reddedilen eşik
   `discussion.md` → Karar 7.
2. **Devir ile kayma tek yüklemin iki yüzü:** `caret_in_dock` imlecin
   görünürlüğünü, doluluk sayısını ve dock'un caret'ini birlikte sürüyor, yani
   tek histerezis ikisini de kapatıyor.
3. **Yeni instance tipi ve yeni `.metal` dosyası yok** (R2). Caret kare başına
   tek quad; şekil uniform'dan geçiyor ve hizalama tuzağı hiç doğmuyor.
4. **`cell.metal` açılmıyor** — yuvarlak köşenin ters çevirmeyle çakışması
   **yok** (ters çevirme temanın zeminini basıyor, boyanmayan köşede duran da
   o). Geçerlilik koşulu: `Cursor::text == tema zemini`. İmlecin altındaki
   metne ayrı bir rol verilirse karar yeniden açılır.
5. **Sayılar türetilir** (R3), `CLAUDE.md`'nin "ikinci bir tasarım sabiti yok"
   kuralı; yan kazanç punto ölçeklemesi.
6. **Odak yalnız `bt-gpu`'ya** (R7). Emsal `set_visible` **değil** (o bir ritim
   kolu, odak `Gate`'e de `setPaused`'a da dokunamaz) — emsal
   `blink.content_frame`'in `AND`'i.
7. **Ayar anahtarı yok** (R8): inen anahtar silinmiyor, emekli oluyor.

### Çözülen karar: odaksız caret sönmez

**Karar (2026-09-19, kullanıcı):** *"Dursun, sabit kalsın."* Odakta olmayan
pencerede blink durur, imleç **görünür** kalır ve içi boşalır. Gereksinim
karşılığı R7.4; `bt-gpu` o biti `cursor.blink && !motion.reduce() && focused`
diye `AND`'liyor.

**Bedeli adıyla:** `bt-gpu`, `Cursor::blink`'in ikinci sahibi olur. Karşılığı
boştaki odaksız pencerenin **saat kurmaması**, yani boşta sıfır kare kuralına
bir kazanç.

## Kapsam Dışı

- **Gölgenin ofseti.** Bu bir **hale**, ışık yönü olan bir gölge değil; ofset
  ayrı bir tasarım sorusu ve kullanıcı "hafif bir dokunuş" istedi.
- **İmleç renginin uygulamadan değişmesi (OSC 12).** 014 rolü ayırdı ve yolu
  açtı; uygulanması ayrı iş.
- **`cursor_motion`'ın yeni stilleri** (Smear/Squash/Phosphor/Arc) — 008'in
  kapsam dışısı olarak duruyor.
- **Kaymanın kendi asimetrik kuralını değiştirmek.** Tek yönlü kayma (düşen
  hedef süzülür, yükselen snap'ler) bu sette tartışılmıyor; histerezis onun
  üstünde çalışıyor.

## Göç

Yok. Ayar şeması, tema biçimi, terminfo, jeton satırı ve kabuk betiği
değişmiyor — `make kur` **zorunlu değil**. Kullanıcı dosyalarına tek bayt
dokunulmuyor.

## Akış

| Phase | İş | Neden bu sırada |
|-------|-----|-----------------|
| phase-1 | Histerezis (`bt-core`) | Shader'a sıfır dokunuş, tek başına sevk edilebilir ve kullanıcıyı günlük rahatsız eden o. `make duman` riskli listesinde değil, yani ucuz |
| phase-2 | Kardeş fragment + yarıçap + hale (`bt-gpu`) | Tek riskli phase (`make shader` + düzen sözleşmesi). Dejenere değerlerle inip sonra açılabilir, yani parite kanıtlanabilir |
| phase-3 | Odak + içi boş imleç | Setin tek crate'ler arası sinyali ve phase-2'nin kenar yolunu açan taraf. Set uzarsa doğal kesme çizgisi |

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | |
| kapı | |

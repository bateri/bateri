//! Bir karenin çizim listesi.
//!
//! `bt-core`'un `frame()` sink'i burayı doğrudan doldurur: grid koordinatı
//! burada piksele çevrilir ve GPU'nun göreceği düzene girer. Renderer "ne
//! çizileceğini" buradan okur, "ne anlama geldiğini" bilmez.
//!
//! Üç liste, iki pipeline: arka planlar (ve imleç) `cell_bg`'nin, glyph'ler ve
//! kural çizgileri `cell`'in. Ayrı durmalarının sebebi çizim sırası —
//! glyph'ler arka planların, kurallar da glyph'lerin **üstüne** gelmek zorunda
//! ve tek listede sıra hücre hücre karışırdı. Glyph ile kuralın ayrı listede
//! olması da aynı cümlenin devamı: ikisi aynı pipeline'dan geçiyor ama üstü
//! çizili, altındaki harften sonra çizilmeli.

use std::mem::offset_of;

use bt_atlas::{Face, RuleKind};
use bt_core::{Cell, Cursor, LinearRgba, UnderlineStyle};

/// `shaders/cell_bg.metal` → `Instance` ile alan alan aynı.
///
/// Crate dışına açılmaz: bu bir GPU bayt düzeni, `bt-core`'un `Cell`'i ise
/// anlam taşıyan grid koordinatı. İkisini aynı tip yapmak hücre modelini
/// shader düzenine çivilemek olurdu.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    /// **Lineer** RGBA. Hedef `BGRA8Unorm_sRGB` ve kodlamayı ROP yapıyor:
    /// shader tarafında ikinci bir gamma düzeltmesi paleti iki kez kodlar.
    /// Kaynağı `bt_core::color::linear_rgba` (`CLAUDE.md` → renk uzayı).
    rgba: [f32; 4],
}

// MSL tarafında float2 8, float4 16 hizalı; Rust'ta hepsi 4 hizalı ama alan
// ofsetleri ve stride örtüşüyor. Bağlanan üç sayı bunlar — biri kayarsa GPU
// baştan sona yanlış renk/konum okur ve belirti sessizdir. (`pos`'un 0'da
// olması `repr(C)`'nin tanımı, assert edilecek bir şey değil.) Bunlar yalnız
// BU tarafı çiviler; MSL tarafının kendi `static_assert`'leri var.
const _: () = assert!(size_of::<Instance>() == 32);
const _: () = assert!(offset_of!(Instance, size) == 8);
const _: () = assert!(offset_of!(Instance, rgba) == 16);

/// `shaders/cell.metal` → `GlyphInstance` ile alan alan aynı.
///
/// **`size` yok, uv boyutu yok**: bu sette her glyph tam bir hücre boyunda
/// (`plan.md` → R1.4, sabit yuva ızgarası) ve ikisi de kare boyunca sabit,
/// yani instance başına değil uniform olarak geçiyorlar. Kazanç yalnız bant
/// genişliği değil: `{pos, size, uv0, rgba}` düzeni Rust'ta 40, MSL'de 48
/// bayt eder (`float4` 16 hizalı, `[f32; 4]` 4) ve ancak sırf hizalama için
/// var olan bir dolgu alanıyla eşlenirdi. Bu hâlde iki taraf dolgusuz
/// örtüşüyor ve stride `Instance`'la aynı 32.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphInstance {
    /// Hücrenin sol üst köşesi, piksel.
    pub(crate) pos: [f32; 2],
    /// Atlastaki yuvanın sol üst köşesi, **normalize** doku koordinatı.
    pub(crate) uv0: [f32; 2],
    /// Ön plan, **lineer** RGBA; `Instance.rgba` ile aynı uzay ve aynı uyarı.
    pub(crate) rgba: [f32; 4],
}

// `Instance` ile aynı gerekçe, aynı ikili bağ.
const _: () = assert!(size_of::<GlyphInstance>() == 32);
const _: () = assert!(offset_of!(GlyphInstance, uv0) == 8);
const _: () = assert!(offset_of!(GlyphInstance, rgba) == 16);

/// `shaders/cell.metal` → `CursorBlock` ile alan alan aynı: imlecin **piksel**
/// dikdörtgeni ve bloğun altında kalan metnin rengi.
///
/// Instance değil **uniform**: kare boyunca tek imleç var ve `cell`
/// pipeline'ından geçen her fragment ona bakıyor. İki değer tek `#[repr(C)]`
/// yapıda çünkü ikisi tek soruyu yanıtlıyor ("bu fragment bloğun altında mı,
/// altındaysa ne renk") ve tek binding tek düzen sözleşmesi demek — ayrı
/// bağlansalardı çivilenecek bir ofset de kalmazdı.
///
/// Dikdörtgen min/max (`x0, y0, x1, y1`), köşe+boyut değil: fragment testi
/// toplama yapmadan iki karşılaştırmaya iniyor. **Görünmez imleç dejenere bir
/// dikdörtgendir** (hepsi sıfır: `x >= 0 && x < 0` hiçbir fragment için doğru
/// değil) — shader'da ikinci bir bayrak yok, çünkü bayrak ile dikdörtgen
/// ayrışabilen iki gerçek olurdu.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorBlock {
    /// `[x0, y0, x1, y1]`, piksel; sol üst başlangıçlı — fragment'in
    /// `[[position]]`'ı ile aynı uzay.
    rect: [f32; 4],
    /// Bloğun altında kalan metnin **lineer** RGBA'sı; `Instance.rgba` ile
    /// aynı uzay ve aynı uyarı. Kaynağı `bt_core::Cursor::text`, yani karar
    /// `bt-core`'un.
    rgba: [f32; 4],
}

// `Instance` ile aynı gerekçe, aynı ikili bağ; MSL tarafının kendi
// `static_assert`'leri var.
const _: () = assert!(size_of::<CursorBlock>() == 32);
const _: () = assert!(offset_of!(CursorBlock, rgba) == 16);

/// Çizilecek bir glyph — **uv'siz**.
///
/// Karakterin hangi yuvaya düştüğü burada bilinmiyor ve bilinmemeli: yuva
/// çözümü atlası `&mut` ödünç alır ve bu liste `Session::frame`'in sink'inde,
/// yani `Renderer`'a hiç dokunmadan doluyor. Çözümü sink'e hoist etmek
/// atlas ödüncünü `draw` boyunca canlı tutar ve ilk glyph'li karede
/// `BorrowMutError` verirdi (`link.rs` `frame`'i tam olarak öyle tutuyor).
/// Bu ayrım bedava bir garanti de veriyor: bütün kare **tek** atlas
/// kuşağıyla çizilir, kuşak sayacı gerekmeden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphCell {
    pub(crate) pos: [f32; 2],
    pub(crate) ch: char,
    /// Hangi font yüzünden rasterize edileceği; `(bold, italic)`'in [`face`]
    /// çevirisi. [`GlyphInstance`] bunu **taşımıyor**: uv0 yuvayı, yuva da
    /// yüzü zaten kodluyor.
    pub(crate) face: Face,
    pub(crate) rgba: [f32; 4],
}

/// Çizilecek bir kural çizgisi — [`GlyphCell`]'in kardeşi ve aynı gerekçeyle
/// uv'siz: yuva çözümü atlas ödüncünün yaşadığı yerde (`encode_glyphs`).
///
/// Yüz taşımıyor çünkü kurallar yüzden bağımsız (kalın metnin altındaki çizgi
/// kalın değildir); çağıran onları her zaman [`Face::Regular`] ile soruyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RuleCell {
    pub(crate) pos: [f32; 2],
    pub(crate) kind: RuleKind,
    pub(crate) rgba: [f32; 4],
}

/// `bt_core`'un SGR bayrakları → `bt_atlas`'ın font yüzü.
///
/// **Çeviri burada, çünkü tek yer.** `bt-atlas` `bt-core`'u görmüyor ve
/// görmemeli: o kenar `alacritty_terminal`'i saf-CoreText crate'ine çekerdi
/// (`CLAUDE.md` → "bağımlılık mimari karardır"); `bt-gpu` ikisini birden gören
/// tek katman. Dört varyantları aynı, **sebepleri ayrı** — biri SGR 1/3
/// semantiği, öteki bir CoreText trait'i. "Aynı görünüyorlar" diye
/// birleştirilirse katman yönü ters döner: birleşik tip ya `bt-core`'a girer
/// (`bt-atlas` onu göremez) ya `bt-atlas`'a (`bt-core` göremez).
fn face(bold: bool, italic: bool) -> Face {
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Bold,
        (false, true) => Face::Italic,
        (true, true) => Face::BoldItalic,
    }
}

/// Alt çizgi çeşidi → kural sprite'ı; [`UnderlineStyle::None`] çizgi istemiyor.
///
/// Beş varyantın beşi de birebir karşılığını buluyor ve `Option` yalnız
/// "çizgi yok"u taşıyor — [`RuleKind`]'ın altıncısı ([`RuleKind::Strike`]) bu
/// çeviriden geçmez, çünkü SGR'de üstü çizili alt çizginin bir çeşidi değil
/// ayrı bir bayrak.
fn rule_kind(underline: UnderlineStyle) -> Option<RuleKind> {
    match underline {
        UnderlineStyle::None => None,
        UnderlineStyle::Single => Some(RuleKind::Single),
        UnderlineStyle::Double => Some(RuleKind::Double),
        UnderlineStyle::Curl => Some(RuleKind::Curl),
        UnderlineStyle::Dotted => Some(RuleKind::Dotted),
        UnderlineStyle::Dashed => Some(RuleKind::Dashed),
    }
}

/// Tek karede çizilecekler.
///
/// Hücre arka planları ve imleç aynı listede yaşar: ikisi de aynı pipeline'la
/// çizilir, sıra çizim sırasıdır (imleç arka planların üstüne gelsin diye
/// sona eklenir). Glyph'ler ayrı listede ve ikinci pipeline'la, imlecin de
/// üstüne çizilir — imleç opak ve altındaki harfi örterdi.
///
/// Uzun ömürlüdür: display link onu ivar'da tutar, her kare `clear` ile
/// yeniden doldurur. Bu yüzden hücre piksel boyutu **alan değil `clear`'ın
/// parametresidir** — kurucuda dondurulsaydı ekran ölçeği değiştiğinde
/// (`windowDidChangeBackingProperties:`) sessizce bayatlardı.
#[derive(Default)]
pub(crate) struct Frame {
    bg: Vec<Instance>,
    glyphs: Vec<GlyphCell>,
    /// Kural çizgileri; glyph'lerle **aynı** pipeline'dan ama onlardan sonra
    /// çizilir (üstü çizili, altındaki harfin üstünden geçmeli).
    rules: Vec<RuleCell>,
    cell_px: (f32, f32),
    /// İmlecin piksel dikdörtgeni ve blok altındaki metin rengi; `cell`
    /// pipeline'ının uniform'u.
    ///
    /// Liste değil **alan**: kare başına tek imleç var ve [`Frame::clear`] onu
    /// dejenereye döndürüyor. Alan olması hareket karesinin de şartı
    /// (`plan.md` → Karar 4): o yol `bg`'yi `bg_count`'a kırpıp
    /// [`Frame::push_cursor`]'ı yeni konumla yeniden çağırıyor, yani ikinci
    /// çağrı birincinin üstüne yazmak zorunda.
    cursor: CursorBlock,
    /// Çizilen **arka plan** instance'ı sayısı; imleç sayılmaz.
    ///
    /// `make duman`'ın `hucre=K` jetonu bunu okur: sink'in hücre ürettiğinin
    /// kanıtı. İmleç sayıya girseydi K boş bir grid'de bile 1 olur ve iddiayı
    /// boşa çıkarırdı. Dikkat: bu bir **CPU** sayacıdır, GPU'nun o hücreleri
    /// boyadığını kanıtlamaz — onu `renderer`'ın offscreen okuma sınaması yapar.
    bg_count: usize,
}

// Tümü `pub(crate)`: `Frame`'i dolduran tek yer `link.rs`, yani bu crate.
// Kare listesi bir GPU ayrıntısıdır; `bt-shell`'in onu görmesi için bir sebep
// yok ve görmezse yanlış hücre boyutuyla dolduramaz.
impl Frame {
    /// Tamponları boşaltır ve bu karenin hücre piksel boyutunu kurar. Ayrılan
    /// yer korunur: kare başına yeniden ayırma yok.
    pub(crate) fn clear(&mut self, cell_px: (u16, u16)) {
        self.bg.clear();
        self.glyphs.clear();
        self.rules.clear();
        self.bg_count = 0;
        // Dikdörtgen de sıfırlanmalı: kalsaydı imlecin sönmesi (`\e[?25l`) ya
        // da geçmişe kayması bloğu ekrandan kaldırır ama **altındaki metnin
        // rengini** eski yerinde bırakırdı — zemin renginde bir harf, yani
        // görünmez bir hücre.
        self.cursor = CursorBlock::default();
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
    }

    /// Sink'in tek girişi: hücrenin arka planı varsa boyanır, mürekkebi varsa
    /// çizilir, kuralı varsa çizilir — üçü de varsa üçü de.
    ///
    /// Bir hücre **ikiye kadar** kural üretir: alt çizgi ve üstü çizili. İkisi
    /// `bt-core`'da ayrı alanlar çünkü SGR'de de ayrılar; aynı hücrede
    /// buluştuklarında ikisi de çizilir.
    pub(crate) fn push(&mut self, cell: Cell) {
        // Dört dalın (arka plan, glyph, alt çizgi, üstü çizili) ortak
        // aritmetiği bir kez: hücre başına dört kez `pos()` çağırmanın kazancı
        // yok ve ayrışabilen dört kopya demek.
        let pos = self.pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            // `bg.len() > bg_count` tam olarak "imleç eklendi" demektir.
            debug_assert_eq!(
                self.bg.len(),
                self.bg_count,
                "arka plan imleçten sonra eklendi: imleç gömülür"
            );
            self.bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
            self.bg_count += 1;
        }
        // Mürekkebi olmayan hücre glyph üretmez: atlasta yuva, tamponda
        // instance ve GPU'da tamamen şeffaf bir dörtlü harcardı. Ayrımı
        // `bt-core` yapıyor (boşluk, gizli metin, geniş karakterin ikinci
        // hücresi hepsi `None`), burada sorulacak bir bayrak yok. Kural
        // dalları buna **bağlı değil**: altı çizili bir boşluk mürekkepsizdir
        // ama çizgisini alır (duman reçetesinde yedi tane var).
        if let Some(ch) = cell.ch {
            self.glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                rgba: cell.fg.to_array(),
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.rules.push(RuleCell {
                pos,
                kind,
                // SGR 58 varsa o, yoksa ön plan (`bt-core` → R3.5).
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                // Üstü çizili SGR 58'i **kullanmaz**: SGR'de üstü çizilinin
                // ayrı bir rengi yok ve `underline_color` adıyla alt çizginin.
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// İmleç bloğu; `bg_count`'a **girmez** ve görünmez imleç çizilmez.
    ///
    /// İki şey birden yazıyor ve bilerek tek çağrıda: bloğun kendisi arka plan
    /// listesine bir dikdörtgen olarak (`rgba` — temanın vurgusu), bloğun
    /// **altında kalan metnin** rengi ise `cell` pipeline'ının uniform'una
    /// (`cursor.text` — `bt-core`'un kararı). İkisi ayrı çağrı olsaydı biri
    /// çağrılıp öteki unutulabilirdi ve belirti sessiz olurdu: imleç doğru
    /// yerde, altındaki harf okunmaz.
    pub(crate) fn push_cursor(&mut self, cursor: Cursor, rgba: LinearRgba) {
        if !cursor.visible {
            return;
        }
        let pos = self.pos(cursor.col, cursor.row);
        self.bg.push(Instance {
            pos,
            size: [self.cell_px.0, self.cell_px.1],
            rgba: rgba.to_array(),
        });
        self.cursor = CursorBlock {
            rect: [
                pos[0],
                pos[1],
                pos[0] + self.cell_px.0,
                pos[1] + self.cell_px.1,
            ],
            rgba: cursor.text.to_array(),
        };
    }

    pub(crate) fn bg_count(&self) -> usize {
        self.bg_count
    }

    /// Bu karenin imleç uniform'u; görünmez imleçte dejenere dikdörtgen.
    pub(crate) fn cursor_block(&self) -> &CursorBlock {
        &self.cursor
    }

    /// Bu karede çizilecek glyph sayısı; `make duman`'ın `glif=G` jetonu.
    /// `bg_count` gibi bir **CPU** sayacı: atlasın o glyph'leri gerçekten
    /// rasterize ettiğini kanıtlamaz, onu offscreen sınaması yapar.
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Bu karede çizilecek kural çizgisi sayısı; `make duman`'ın `kural=R`
    /// jetonu.
    ///
    /// **Jetonun sınırının tek sahibi burası** — okuyan üç yer (`app.rs`'in
    /// duman kapısı, `Renderer::last_rule_count`, offscreen sınamalar) buraya
    /// işaret ediyor; dört kopya olsaydı bekçi yeniden adlandırıldığında üçü
    /// sessizce bayatlardı. Sınır iki katlı: (1) `bg_count` ve `glyph_count`
    /// gibi bir **CPU** sayacı, GPU'nun o çizgileri boyadığını kanıtlamaz;
    /// (2) **stil ayrımını göremez** — beş çeşidi de düz çizgi olarak çizen
    /// bir kod da aynı R'yi basar. Birincisini `rule_band_is_not_uniform_along_x`
    /// ve `sgr58_color_differs_from_foreground` kapatıyor, ikincisini o kıvrım
    /// sınaması ile `bt-core`'un `smoke_shell_distinguishes_five_styles`'ı.
    pub(crate) fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Bu karenin hücre piksel boyutu; glyph dörtlüsünün boyu.
    ///
    /// Instance başına taşınmıyor (bkz. [`GlyphInstance`]), uniform olarak
    /// gidiyor — kare boyunca tek değer. Dönüş `[f32; 2]`: tek tüketicisi
    /// onu shader'a öyle geçiriyor.
    pub(crate) fn cell_px(&self) -> [f32; 2] {
        [self.cell_px.0, self.cell_px.1]
    }

    pub(crate) fn bg_instances(&self) -> &[Instance] {
        &self.bg
    }

    pub(crate) fn glyphs(&self) -> &[GlyphCell] {
        &self.glyphs
    }

    pub(crate) fn rules(&self) -> &[RuleCell] {
        &self.rules
    }

    /// Grid koordinatının sol üst köşesi, piksel — **üç listenin ortak
    /// aritmetiği**.
    ///
    /// [`Frame::push`] onu hücre başına bir kez çağırıyor, yani bekçi artık
    /// push edilen **her** hücrede koşuyor: `clear` çağrılmadan push edilen
    /// hücre sıfır boyutlu doğar ve ekranda sessizce kaybolur. Tek çağrı
    /// olduğu için formülün dallara kopyalanma ihtimali de kalmadı; imleç
    /// yolu ([`Frame::push_cursor`]) aynı fonksiyondan geçen ikinci çağıran.
    fn pos(&self, col: u16, row: u16) -> [f32; 2] {
        let (w, h) = self.cell_px;
        debug_assert!(w > 0.0 && h > 0.0, "clear(cell_px) çağrılmadı");
        [f32::from(col) * w, f32::from(row) * h]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uçlar bilerek: `0.0`/`1.0` sRGB transfer fonksiyonunun sabit noktaları,
    // yani bu sınamalar renk uzayından bağımsız. Uzayı sınayan yer
    // `renderer.rs` → `cell_bg_paints_pixels_on_the_gpu`.
    //
    // Gömülü temanın iki ayrık rengi; adları rolleri değil kaynakları
    // söylüyor. Burada bakılan şey renk değil düzen, o yüzden renk uydurmaya
    // (`LinearRgba::from_srgb`) gerek yok — `renderer.rs`'in offscreen
    // sınamaları onu üç ayrık ton gerektirdikleri için kullanıyor.
    const BG: LinearRgba = bt_core::Theme::BATERI.background_linear();
    const CURSOR: LinearRgba = bt_core::Theme::BATERI.accent_linear();
    /// Blok altındaki metnin rengi; üretimde temanın zemini (`bt-core` →
    /// `Cursor::text`). Burada bloğun renginden **ayrık** olması yetiyor.
    const TEXT: LinearRgba = BG;

    fn cursor(col: u16, row: u16, visible: bool) -> Cursor {
        Cursor {
            col,
            row,
            visible,
            text: TEXT,
        }
    }

    fn bg_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn frame_bg_count_excludes_cursor() {
        let mut frame = Frame::default();
        frame.clear((9, 18));

        frame.push(bg_cell(0, 0));
        frame.push(bg_cell(1, 0));
        frame.push_cursor(cursor(5, 2, true), CURSOR);

        // Üç dikdörtgen çizilir ama `hucre=K` yalnız ikisini sayar.
        assert_eq!(frame.bg_instances().len(), 3);
        assert_eq!(frame.bg_count(), 2);

        frame.clear((9, 18));
        assert_eq!(frame.bg_count(), 0);
        assert!(frame.bg_instances().is_empty());
    }

    #[test]
    fn invisible_cursor_is_not_drawn() {
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push_cursor(cursor(0, 0, false), CURSOR);
        assert!(frame.bg_instances().is_empty());
        // Uniform da dokunulmadan kalır: dejenere dikdörtgen "blok yok"
        // demenin tek yolu, shader'da ikinci bir bayrak yok.
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn cursor_block_covers_its_cell_and_clears_with_the_frame() {
        // Dikdörtgen bloğun **kendi** instance'ıyla aynı hücreye oturmalı:
        // ayrışsalardı blok bir yerde, altındaki metnin rengi başka bir yerde
        // olurdu ve ikisi de sessizce yanlış çizerdi.
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push_cursor(cursor(3, 2, true), CURSOR);

        let block = *frame.cursor_block();
        assert_eq!(block.rect, [27.0, 36.0, 36.0, 54.0]);
        assert_eq!(
            block.rgba,
            TEXT.to_array(),
            "metin rengi `Cursor`'dan gelir"
        );
        let instance = frame.bg_instances().last().expect("blok instance'ı");
        assert_eq!(
            [
                instance.pos[0],
                instance.pos[1],
                instance.pos[0] + instance.size[0],
                instance.pos[1] + instance.size[1],
            ],
            block.rect,
            "dikdörtgen bloğun instance'ıyla ayrıştı"
        );

        // `clear` uniform'u da sıfırlar: sönen imleç (`\e[?25l`) bloğu
        // kaldırır ama rengi eski hücrede bırakırsa orası görünmez olur.
        frame.clear((9, 18));
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn clear_updates_cell_size() {
        // `cell_px`'in `clear`'ın parametresi olmasının tek sebebi bu: alan
        // olsaydı ekran ölçeği değişince bayatlardı ve hiçbir sınama görmezdi.
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [9.0, 18.0]);

        frame.clear((18, 36));
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [18.0, 36.0]);
        assert_eq!(frame.bg_instances()[0].size, [18.0, 36.0]);
    }

    #[test]
    fn grid_coords_convert_to_pixels() {
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push(bg_cell(3, 2));
        assert_eq!(
            frame.bg_instances()[0],
            Instance {
                pos: [27.0, 36.0],
                size: [9.0, 18.0],
                rgba: BG.to_array(),
            }
        );
    }

    #[test]
    fn inkless_cell_yields_background_without_glyph() {
        // `hucre=K` ile `glif=G`'yi ayıran satır bu: `" bateri "` sekiz arka
        // planlı hücredir ama altı glyph'tir. İkisi tek sayaçtan okunsaydı
        // duman kapısı ikisinden birini hiç sormamış olurdu.
        let mut frame = Frame::default();
        frame.clear((8, 16));
        frame.push(bg_cell(0, 0)); // mürekkepsiz
        frame.push(Cell {
            col: 1,
            row: 0,
            ch: Some('b'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        });
        // Arka planı olmayan ama mürekkebi olan hücre: yalnız glyph listesine.
        frame.push(Cell {
            col: 2,
            row: 0,
            ch: Some('a'),
            fg: CURSOR,
            bg: None,
            ..Default::default()
        });

        assert_eq!(frame.bg_count(), 2);
        assert_eq!(frame.glyph_count(), 2);
        assert_eq!(
            frame.glyphs()[1],
            GlyphCell {
                pos: [16.0, 0.0],
                ch: 'a',
                face: Face::Regular,
                rgba: CURSOR.to_array(),
            }
        );

        frame.clear((8, 16));
        assert_eq!(frame.glyph_count(), 0);
    }

    #[test]
    fn cell_yields_up_to_two_rules() {
        // Beş iddia, beşi de sessizce bozulabilir: kuralsız hücre kural
        // üretmez, altı çizili **mürekkepsiz** hücre üretir (`ch: None` kuralı
        // düşürmez), alt çizgi rengi SGR 58'den gelir, üstü çizili hep ön
        // plandan, ikisi aynı hücrede buluşabilir — ve `clear` kural listesini
        // de boşaltır (üç listenin üçü de aynı çağrıda sıfırlanmalı).
        let mut frame = Frame::default();
        frame.clear((8, 16));

        frame.push(bg_cell(0, 0));
        assert_eq!(frame.rule_count(), 0, "kuralsız hücre kural üretti");

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        frame.push(Cell {
            col: 1,
            row: 0,
            fg: CURSOR,
            underline: UnderlineStyle::Curl,
            underline_color: Some(red),
            strikeout: true,
            ..Default::default()
        });

        assert_eq!(frame.rule_count(), 2);
        assert_eq!(frame.glyph_count(), 0, "kural hücresi mürekkep üretmedi");
        assert_eq!(
            frame.rules(),
            [
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Curl,
                    rgba: red.to_array(),
                },
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Strike,
                    rgba: CURSOR.to_array(),
                },
            ]
        );

        frame.clear((8, 16));
        assert_eq!(frame.rule_count(), 0);
    }

    #[test]
    fn sgr_flags_translate_to_four_faces() {
        // Çevirinin tek yeri burası ve dört kolun ikisi karıştığında belirti
        // "eğik metin kalın çiziliyor" olur — hiçbir sayaç görmez.
        assert_eq!(face(false, false), Face::Regular);
        assert_eq!(face(true, false), Face::Bold);
        assert_eq!(face(false, true), Face::Italic);
        assert_eq!(face(true, true), Face::BoldItalic);
    }
}

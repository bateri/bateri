//! Bir karenin çizim listesi.
//!
//! `bt-core`'un `frame()` sink'i burayı doğrudan doldurur: grid koordinatı
//! burada piksele çevrilir ve GPU'nun göreceği düzene girer. Renderer "ne
//! çizileceğini" buradan okur, "ne anlama geldiğini" bilmez.
//!
//! İki liste, iki pipeline: arka planlar (ve imleç) `cell_bg`'nin, glyph'ler
//! `cell`'in. Ayrı durmalarının sebebi çizim sırası — glyph'ler arka planların
//! **üstüne** gelmek zorunda ve tek listede sıra hücre hücre karışırdı.

use std::mem::offset_of;

use bt_core::{Cell, Cursor, LinearRgba};

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
    pub(crate) rgba: [f32; 4],
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
    cell_px: (f32, f32),
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
        self.bg_count = 0;
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
    }

    /// Sink'in tek girişi: hücrenin arka planı varsa boyanır, mürekkebi varsa
    /// çizilir, ikisi de varsa ikisi de.
    pub(crate) fn push(&mut self, cell: Cell) {
        if let Some(bg) = cell.bg {
            // `bg.len() > bg_count` tam olarak "imleç eklendi" demektir.
            debug_assert_eq!(
                self.bg.len(),
                self.bg_count,
                "arka plan imleçten sonra eklendi: imleç gömülür"
            );
            self.bg.push(Instance {
                pos: self.pos(cell.col, cell.row),
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
            self.bg_count += 1;
        }
        // Mürekkebi olmayan hücre glyph üretmez: atlasta yuva, tamponda
        // instance ve GPU'da tamamen şeffaf bir dörtlü harcardı. Ayrımı
        // `bt-core` yapıyor (boşluk, gizli metin, geniş karakterin ikinci
        // hücresi hepsi `None`), burada sorulacak bir bayrak yok.
        if let Some(ch) = cell.ch {
            self.glyphs.push(GlyphCell {
                pos: self.pos(cell.col, cell.row),
                ch,
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// İmleç bloğu; `bg_count`'a **girmez** ve görünmez imleç çizilmez.
    pub(crate) fn push_cursor(&mut self, cursor: Cursor, rgba: LinearRgba) {
        if !cursor.visible {
            return;
        }
        self.bg.push(Instance {
            pos: self.pos(cursor.col, cursor.row),
            size: [self.cell_px.0, self.cell_px.1],
            rgba: rgba.to_array(),
        });
    }

    pub(crate) fn bg_count(&self) -> usize {
        self.bg_count
    }

    /// Bu karede çizilecek glyph sayısı; `make duman`'ın `glif=G` jetonu.
    /// `bg_count` gibi bir **CPU** sayacı: atlasın o glyph'leri gerçekten
    /// rasterize ettiğini kanıtlamaz, onu offscreen sınaması yapar.
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
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

    /// Grid koordinatının sol üst köşesi, piksel — **iki listenin ortak
    /// aritmetiği**.
    ///
    /// Bekçi burada duruyor ki iki yolu birden korusun: `clear` çağrılmadan
    /// push edilen hücre sıfır boyutlu doğar ve ekranda sessizce kaybolur.
    /// Formül arka plan dalında kopyalanmış olsaydı yalnız mürekkep taşıyan
    /// bir kare bu bekçinin dışında kalırdı.
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
    // Paletin iki ayrık sabiti; adları rolleri değil kaynakları söylüyor.
    // Burada bakılan şey renk değil düzen, o yüzden renk uydurmaya
    // (`LinearRgba::from_srgb`) gerek yok — `renderer.rs`'in offscreen
    // sınamaları onu üç ayrık ton gerektirdikleri için kullanıyor.
    const BG: LinearRgba = bt_core::DEFAULT_BG;
    const CURSOR: LinearRgba = bt_core::DEFAULT_CURSOR;

    fn bg_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            ch: None,
            fg: CURSOR,
            bg: Some(BG),
        }
    }

    #[test]
    fn frame_bg_count_excludes_cursor() {
        let mut frame = Frame::default();
        frame.clear((9, 18));

        frame.push(bg_cell(0, 0));
        frame.push(bg_cell(1, 0));
        frame.push_cursor(
            Cursor {
                col: 5,
                row: 2,
                visible: true,
            },
            CURSOR,
        );

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
        frame.push_cursor(
            Cursor {
                col: 0,
                row: 0,
                visible: false,
            },
            CURSOR,
        );
        assert!(frame.bg_instances().is_empty());
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
        });
        // Arka planı olmayan ama mürekkebi olan hücre: yalnız glyph listesine.
        frame.push(Cell {
            col: 2,
            row: 0,
            ch: Some('a'),
            fg: CURSOR,
            bg: None,
        });

        assert_eq!(frame.bg_count(), 2);
        assert_eq!(frame.glyph_count(), 2);
        assert_eq!(
            frame.glyphs()[1],
            GlyphCell {
                pos: [16.0, 0.0],
                ch: 'a',
                rgba: CURSOR.to_array(),
            }
        );

        frame.clear((8, 16));
        assert_eq!(frame.glyph_count(), 0);
    }
}

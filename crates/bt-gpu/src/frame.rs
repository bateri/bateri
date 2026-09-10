//! Bir karenin çizim listesi.
//!
//! `bt-core`'un `frame()` sink'i burayı doğrudan doldurur: grid koordinatı
//! burada piksele çevrilir ve GPU'nun göreceği düzene girer. Renderer "ne
//! çizileceğini" buradan okur, "ne anlama geldiğini" bilmez.

use std::mem::offset_of;

use bt_core::{CellBg, Cursor};

/// `shaders/cell_bg.metal` → `Instance` ile alan alan aynı.
///
/// Crate dışına açılmaz: bu bir GPU bayt düzeni, `bt-core`'un `CellBg`'si ise
/// anlam taşıyan grid koordinatı. İkisini aynı tip yapmak hücre modelini
/// shader düzenine çivilemek olurdu.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
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

/// Tek karede çizilecek dikdörtgenler.
///
/// Hücre arka planları ve imleç aynı listede yaşar: ikisi de aynı pipeline'la
/// çizilir, sıra çizim sırasıdır (imleç arka planların üstüne gelsin diye
/// sona eklenir).
///
/// Uzun ömürlüdür: display link onu ivar'da tutar, her kare `clear` ile
/// yeniden doldurur. Bu yüzden hücre piksel boyutu **alan değil `clear`'ın
/// parametresidir** — kurucuda dondurulsaydı ekran ölçeği değiştiğinde
/// (`windowDidChangeBackingProperties:`) sessizce bayatlardı.
#[derive(Default)]
pub struct Frame {
    instances: Vec<Instance>,
    cell_px: (f32, f32),
    /// Çizilen **arka plan** instance'ı sayısı; imleç sayılmaz.
    ///
    /// `make duman`'ın `hucre=K` jetonu bunu okuyacak (phase-3'te doğuyor):
    /// sink'in hücre ürettiğinin kanıtı. İmleç sayıya girseydi K, boş bir
    /// grid'de bile 1 olur ve iddiayı boşa çıkarırdı. Dikkat: bu bir **CPU**
    /// sayacıdır, GPU'nun o hücreleri boyadığını kanıtlamaz — onu
    /// `renderer`'ın offscreen okuma sınaması yapar.
    bg_count: usize,
}

// Kurucular `pub`: bugün `Frame`'i dolduracak tek yer crate DIŞI olurdu
// (`bt-shell`), çünkü display link phase-3'te doğuyor. O gün sahiplik
// `bt-gpu`'nun içine geçince bunlar `pub(crate)`'e daralır — phase-3
// checklist'inde yazılı.
impl Frame {
    /// Tamponu boşaltır ve bu karenin hücre piksel boyutunu kurar. Ayrılan
    /// yer korunur: kare başına yeniden ayırma yok.
    pub fn clear(&mut self, cell_px: (u16, u16)) {
        self.instances.clear();
        self.bg_count = 0;
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
    }

    pub fn push_bg(&mut self, cell: CellBg) {
        // `instances.len() > bg_count` tam olarak "imleç eklendi" demektir.
        debug_assert_eq!(
            self.instances.len(),
            self.bg_count,
            "arka plan imleçten sonra eklendi: imleç gömülür"
        );
        self.instances
            .push(self.hucre(cell.col, cell.row, cell.rgba));
        self.bg_count += 1;
    }

    /// İmleç bloğu; `bg_count`'a **girmez** ve görünmez imleç çizilmez.
    pub fn push_cursor(&mut self, cursor: Cursor, rgba: [f32; 4]) {
        if !cursor.visible {
            return;
        }
        self.instances
            .push(self.hucre(cursor.col, cursor.row, rgba));
    }

    pub fn bg_count(&self) -> usize {
        self.bg_count
    }

    pub(crate) fn instances(&self) -> &[Instance] {
        &self.instances
    }

    fn hucre(&self, col: u16, row: u16, rgba: [f32; 4]) -> Instance {
        let (w, h) = self.cell_px;
        // `clear` çağrılmadan push edilirse hücreler sıfır boyutlu doğar ve
        // ekranda sessizce kaybolur.
        debug_assert!(w > 0.0 && h > 0.0, "clear(cell_px) çağrılmadı");
        Instance {
            pos: [f32::from(col) * w, f32::from(row) * h],
            size: [w, h],
            rgba,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIRMIZI: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const MAVI: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    fn hucre(col: u16, row: u16) -> CellBg {
        CellBg {
            col,
            row,
            rgba: KIRMIZI,
        }
    }

    #[test]
    fn frame_bg_count_imleci_saymaz() {
        let mut frame = Frame::default();
        frame.clear((9, 18));

        frame.push_bg(hucre(0, 0));
        frame.push_bg(hucre(1, 0));
        frame.push_cursor(
            Cursor {
                col: 5,
                row: 2,
                visible: true,
            },
            MAVI,
        );

        // Üç dikdörtgen çizilir ama `hucre=K` yalnız ikisini sayar.
        assert_eq!(frame.instances().len(), 3);
        assert_eq!(frame.bg_count(), 2);

        frame.clear((9, 18));
        assert_eq!(frame.bg_count(), 0);
        assert!(frame.instances().is_empty());
    }

    #[test]
    fn gorunmez_imlec_cizilmez() {
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push_cursor(
            Cursor {
                col: 0,
                row: 0,
                visible: false,
            },
            MAVI,
        );
        assert!(frame.instances().is_empty());
    }

    #[test]
    fn clear_hucre_boyutunu_gunceller() {
        // `cell_px`'in `clear`'ın parametresi olmasının tek sebebi bu: alan
        // olsaydı ekran ölçeği değişince bayatlardı ve hiçbir sınama görmezdi.
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push_bg(hucre(1, 1));
        assert_eq!(frame.instances()[0].pos, [9.0, 18.0]);

        frame.clear((18, 36));
        frame.push_bg(hucre(1, 1));
        assert_eq!(frame.instances()[0].pos, [18.0, 36.0]);
        assert_eq!(frame.instances()[0].size, [18.0, 36.0]);
    }

    #[test]
    fn grid_koordinati_piksele_cevrilir() {
        let mut frame = Frame::default();
        frame.clear((9, 18));
        frame.push_bg(hucre(3, 2));
        assert_eq!(
            frame.instances()[0],
            Instance {
                pos: [27.0, 36.0],
                size: [9.0, 18.0],
                rgba: KIRMIZI,
            }
        );
    }
}

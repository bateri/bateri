//! Farenin **jest defteri**: hangi basış uygulamaya raporlandı, hangisi
//! seçim başlattı, hareket raporunun kısması nerede — `NSEvent` görmeyen,
//! sınanan bir struct.
//!
//! Defter bir dönem `BateriView`'ın `impl` gövdesindeydi ve `define_class!`
//! komşusu kod sınanamıyordu: rotayı kilitleyen dört geçiş (basış,
//! sürükleme, bırakma, kayıp bırakma) yalnız gözle doğrulanmıştı (020 set
//! kapısının waive'i, `.tasks/031-fare-ile-secim/`'te kapandı). Burada
//! AppKit yok: view olayı düğmeye, tıklama sayısına ve Shift'e çevirip
//! defterin cevabına göre `Session`'ı çağırıyor.
//!
//! **Karar hâlâ `bt-core`'da.** Jestin uygulamanın mı terminalin mi olduğunu
//! kip söylüyor ve kip `Term`'de (`Session::mouse_button` → [`Click`]);
//! defter o cevabı **kaydediyor**, yeniden türetmiyor. Bu yüzden basış iki
//! adım: [`Gesture::begin_press`] (cevaptan önce, bayat izi siler) ve
//! [`Gesture::pressed`] (cevaptan sonra, yeni izi yazar).

use bt_core::{Click, MouseButton, SelectKind, SelectionPoint};

/// Jestin durumu. `Copy`: view onu bir `Cell`'de tutuyor ve her olayda
/// al-değiştir-koy yapıyor — `RefCell`'in ödünç alma paniği `Session`
/// çağrısının ortasında riske girmesin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Gesture {
    /// Sol tuş basılı ve seçim bu basışla başladı mı.
    ///
    /// Çapanın **kendisi** burada değil: `bt-core`'da grid mutlağında
    /// (`Session::set_selection`). Kalan soru yalnız "sürükleme sürüyor mu":
    /// basışsız bir `mouseDragged:` eski seçimin ucunu taşımasın.
    dragging: bool,
    /// Seçim sürüklemesi **dock'un** giriş satırında mı (031 phase-4).
    ///
    /// Hedef basışta kilitleniyor, raporun rotası gibi: sürükleme bandın
    /// dışına taşsa da dock'un seçimini büyütüyor (satırın içine kırpılarak),
    /// ızgaraya geçmiyor. Anlamı yalnız `dragging` kuruluyken var.
    dock: bool,
    /// Basışı **uygulamaya raporlanmış** düğmeler, düğme başına bir bit
    /// ([`button_bit`]).
    ///
    /// Rota basışta kilitleniyor (020 R6): Shift her olayda okunsaydı
    /// sürüklemenin ortasında Shift'i bırakmak seçim jestini rapor jestine
    /// çevirirdi. Bırakma bu yüzden kipi değil **bu biti** soruyor.
    /// `dragging`'in yanında ve onun içinde değil: sol tuşla seçim sürerken
    /// sağ tuşa basmak ikisini **aynı anda** doğuruyor. Bitmask, çünkü üç
    /// düğme birden basılı tutulabilir.
    sent: u8,
    /// Hareket raporunun son gittiği hücre — kısmanın çentiği.
    ///
    /// Rapor **hücre başına** en çok bir kez gitmeli; karşılaştırma `bt-core`
    /// çağrısından **önce** koşuyor, yani aynı hücrede kalan hareket `Term`
    /// kilidine hiç uğramıyor. Ölçü **görünür pencere** hücresi, `half`
    /// girmiyor — rapor hücre çözünürlüğünde. Basış ve bırakma çentiği
    /// yalnız **raporlandıklarında** tazeliyor ([`Gesture::stamp`]): ölçüt
    /// "burası uygulamaya bildirildi".
    notch: Option<(u16, u16)>,
}

/// Terminalin basıştan yapacağı iş.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    /// Yeni seçim, tıklama sayısının adımıyla (sürüklemesiz tek tık boş
    /// seçimdir ve eski vurguyu kaldırır).
    Select(SelectKind),
    /// Shift+tıklama: var olan seçimin ucunu taşı (`Session::extend_selection`).
    Extend,
}

/// Basılı sürüklemenin yolu — basışta kilitlenen rotadan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drag {
    /// Basış raporlandı: hareket de rapor.
    Report,
    /// Basış ızgarada seçim başlattı: seçimin ucu taşınır.
    Select,
    /// Basış dock'ta seçim başlattı: dock seçiminin ucu taşınır.
    SelectDock,
    /// İkisi de değil (sağ/orta tuşun terminalde jesti yok, basışsız
    /// sürükleme): olay düşer.
    Ignore,
}

/// Bırakmanın yolu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Release {
    /// Basış raporlandı: bırakma da raporlanır (takılı düğme kalmasın).
    Report,
    /// Seçim jestinin sonu ya da jestsiz bırakma: gidecek bir şey yok.
    Done,
}

impl Gesture {
    /// Yeni basış yeni jest: aynı düğmenin **bayat** izi burada iner.
    ///
    /// Kayıp bir `mouseUp:` (sürüklemenin ortasında bir modal, bir sistem
    /// jesti) bit ya da `dragging` bırakabilir; inmeseydi kip bu arada
    /// kapandığında basış seçim olur, bırakma bayat biti bulup rapor yolunu
    /// seçerdi — ya da raporlanan yeni basışın yanında bayat `dragging`
    /// sonraki her kaydırmada eski seçimi uzatırdı.
    pub(crate) fn begin_press(&mut self, button: MouseButton) {
        self.sent &= !button_bit(button);
        if button == MouseButton::Left {
            self.dragging = false;
            self.dock = false;
        }
    }

    /// Sol tuşun basışı dock'un giriş satırında: jest **terminalin** (fare
    /// kipi dock'a hiç uygulanmıyor — bant uygulamanın ekranı değil) ve
    /// tıklama sayısı ile Shift ızgaradaki kuralla okunuyor
    /// ([`Gesture::pressed`]). Çağıran önce [`Gesture::begin_press`]'i
    /// çağırmış olmalı.
    pub(crate) fn pressed_dock(&mut self, clicks: isize, shift: bool) -> Press {
        self.dragging = true;
        self.dock = true;
        if shift {
            Press::Extend
        } else {
            Press::Select(click_kind(clicks))
        }
    }

    /// `bt-core`'un cevabını ([`Click`]) deftere yazar ve terminalin işini
    /// söyler. `clicks` AppKit'in `clickCount`'u ([`click_kind`]).
    ///
    /// **Shift sayıdan önce gelir**: Shift+tıklama seçimi uzatır, tıklama
    /// sayısı ne olursa olsun, ve bu iki kipte de aynı kural — fare kipinde
    /// Shift'li basış zaten seçime düşüyor (`bt-core`'un arbitrajı) ve orada
    /// seçimin tek yolu Shift (031 Karar 6). Seçim yoksa uzatma tıklanan
    /// noktadan başlar; o karar `Session::extend_selection`'ın.
    ///
    /// Seçimi yalnız **sol** tuş başlatır: sağ ya da orta tık beklenmedik bir
    /// vurgu üretirdi. Rapor gittiyse `dragging` kurulmaz, yoksa
    /// `mouseDragged:` eski seçimin ucunu büyütürdü.
    pub(crate) fn pressed(
        &mut self,
        button: MouseButton,
        answer: Click,
        clicks: isize,
        shift: bool,
    ) -> Option<Press> {
        match answer {
            Click::Sent => {
                self.sent |= button_bit(button);
                None
            }
            Click::Select if button == MouseButton::Left => {
                self.dragging = true;
                Some(if shift {
                    Press::Extend
                } else {
                    Press::Select(click_kind(clicks))
                })
            }
            Click::Select | Click::Ignored => None,
        }
    }

    /// Basılı sürükleme: rota basışta kilitlendi ve burada yeniden
    /// sorulmuyor. Kilidin iki yarısı da okunuyor ve rapor önce gelir —
    /// ikisi aynı anda kurulu olabilir (sol seçim sürerken sağ tuşa basmak),
    /// ama bit **düğme başına**.
    pub(crate) fn dragged(&self, button: MouseButton) -> Drag {
        if self.sent & button_bit(button) != 0 {
            Drag::Report
        } else if button == MouseButton::Left && self.dragging && self.dock {
            Drag::SelectDock
        } else if button == MouseButton::Left && self.dragging {
            Drag::Select
        } else {
            Drag::Ignore
        }
    }

    /// Bırakma: basış raporlandıysa bit iner ve rapor gider; değilse sol
    /// tuşun seçim jesti biter (seçim ekranda kalır, Cmd-C onu kopyalar).
    pub(crate) fn released(&mut self, button: MouseButton) -> Release {
        let bit = button_bit(button);
        if self.sent & bit == 0 {
            if button == MouseButton::Left {
                self.dragging = false;
            }
            return Release::Done;
        }
        self.sent &= !bit;
        Release::Report
    }

    /// Kayıp bir `mouseUp:`'ın uygulamada basılı bıraktığı düğmeler; defter
    /// onları unutur, çağıran her biri için bırakmayı **raporlamak** zorunda
    /// (uygulama düğmeyi hâlâ basılı sanıyor).
    ///
    /// Kanıtı çağıranın selector'ı: AppKit `mouseMoved:`'ı yalnız hiçbir
    /// düğme basılı değilken gönderiyor, yani orada kurulu bir bit tek bir
    /// şey demek — bırakma bu view'a hiç varmadı.
    pub(crate) fn take_lost_releases(&mut self) -> impl Iterator<Item = MouseButton> {
        let lost = std::mem::take(&mut self.sent);
        [MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .into_iter()
            .filter(move |&button| lost & button_bit(button) != 0)
    }

    /// Sol tuş sistemde **basılı değil** ama `dragging` kurulu: bırakma bu
    /// view'a hiç varmadı. Bayat bayrak iner; inmeseydi tuşsuz her kaydırma
    /// eski seçimi sessizce uzatır, sonraki Cmd-C onu kopyalardı.
    pub(crate) fn lost_drag(&mut self) {
        self.dragging = false;
    }

    /// **Izgarada** seçim sürüklemesi sürüyor mu — kaydırmanın ucu fareye
    /// taşıma sorusu. Dock'un sürüklemesi burada `false`: dock kaymıyor,
    /// yani pencere kayınca taşınacak bir uç da yok.
    pub(crate) fn dragging(&self) -> bool {
        self.dragging && !self.dock
    }

    /// Çentiği taze hücreye taşır ve hücrenin **değiştiğini** söyler — ilk
    /// görüşte `true`, tekrarda `false`. Yarı okunmuyor.
    pub(crate) fn moved_to(&mut self, cell: SelectionPoint) -> bool {
        let now = (cell.col, cell.row);
        self.notch.replace(now) != Some(now)
    }

    /// Raporlanan basış ya da bırakmanın hücresini çentiğe damgalar: aynı
    /// hücrede gelecek ilk hareket ikinci bir rapor üretmesin.
    pub(crate) fn stamp(&mut self, cell: SelectionPoint) {
        self.moved_to(cell);
    }
}

/// AppKit'in `clickCount`'undan seçimin adımı: 1 harf, 2 kelime, 3 satır.
///
/// Üçten büyük sayı **satırda kalıyor**: dörtlü tıklama (akıllı seçim)
/// kapsam dışı ve hızlı tıklayan kullanıcının dördüncü tıkı satırı
/// bırakmamalı. Sıfır ya da eksi (sentetik olay) tek tık sayılıyor.
pub(crate) fn click_kind(clicks: isize) -> SelectKind {
    match clicks {
        2 => SelectKind::Word,
        3.. => SelectKind::Line,
        _ => SelectKind::Simple,
    }
}

/// Düğmenin defterdeki biti. Raporun düğme kodundan ([`bt_core`] içinde)
/// **ayrı**: bu bir maske, o bir bayt değeri.
fn button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 4,
    }
}

#[cfg(test)]
mod tests {
    use bt_core::CellHalf;

    use super::*;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    /// Sol tuşla basış: önce bayat iz iner, sonra `bt-core`'un cevabı yazılır.
    fn press(gesture: &mut Gesture, answer: Click, clicks: isize, shift: bool) -> Option<Press> {
        gesture.begin_press(LEFT);
        gesture.pressed(LEFT, answer, clicks, shift)
    }

    #[test]
    fn a_selecting_press_drags_the_selection_until_release() {
        let mut gesture = Gesture::default();
        // Sürüklemeden önce basış yok: olay düşer.
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
        assert_eq!(
            press(&mut gesture, Click::Select, 1, false),
            Some(Press::Select(SelectKind::Simple))
        );
        assert_eq!(gesture.dragged(LEFT), Drag::Select);
        assert_eq!(gesture.released(LEFT), Release::Done);
        // Bırakmadan sonra sürükleme yok.
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
        assert!(!gesture.dragging());
    }

    #[test]
    fn a_reported_press_is_reported_to_its_release() {
        let mut gesture = Gesture::default();
        assert_eq!(press(&mut gesture, Click::Sent, 1, false), None);
        assert_eq!(gesture.dragged(LEFT), Drag::Report);
        assert!(!gesture.dragging(), "rapor jesti seçim sürüklemesi değil");
        assert_eq!(gesture.released(LEFT), Release::Report);
        // Bit indi: ikinci bir bırakma raporlanmaz.
        assert_eq!(gesture.released(LEFT), Release::Done);
    }

    #[test]
    fn the_click_count_picks_the_step() {
        let mut gesture = Gesture::default();
        for (clicks, kind) in [
            (0, SelectKind::Simple),
            (1, SelectKind::Simple),
            (2, SelectKind::Word),
            (3, SelectKind::Line),
            (4, SelectKind::Line),
            (7, SelectKind::Line),
        ] {
            assert_eq!(
                press(&mut gesture, Click::Select, clicks, false),
                Some(Press::Select(kind)),
                "{clicks}"
            );
        }
    }

    #[test]
    fn a_shift_click_extends_whatever_the_count() {
        // Fare kipinde de aynı: Shift'li basış `bt-core`'da seçime düşüyor
        // (`Click::Select`, bekçisi `input::tests`'te) ve defter onu rapor
        // değil uzatma olarak okuyor.
        let mut gesture = Gesture::default();
        for clicks in 1..=3 {
            assert_eq!(
                press(&mut gesture, Click::Select, clicks, true),
                Some(Press::Extend)
            );
            // Uzatma da sürüklenir: Shift+tıklayıp sürüklemek ucu taşır.
            assert_eq!(gesture.dragged(LEFT), Drag::Select);
        }
    }

    #[test]
    fn only_the_left_button_selects() {
        let mut gesture = Gesture::default();
        gesture.begin_press(RIGHT);
        assert_eq!(gesture.pressed(RIGHT, Click::Select, 2, false), None);
        assert_eq!(gesture.dragged(RIGHT), Drag::Ignore);
        assert!(!gesture.dragging());
        // Rapor gitmeyen basış iz bırakmaz.
        assert_eq!(press(&mut gesture, Click::Ignored, 1, false), None);
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
    }

    #[test]
    fn a_lost_release_is_reported_and_forgotten() {
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Sent, 1, false);
        gesture.begin_press(RIGHT);
        gesture.pressed(RIGHT, Click::Sent, 1, false);
        // `mouseUp:` hiç varmadı; düğmesiz hareket ikisini de serbest bırakır.
        let lost: Vec<_> = gesture.take_lost_releases().collect();
        assert_eq!(lost, [LEFT, RIGHT]);
        assert_eq!(gesture.take_lost_releases().count(), 0);
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
    }

    #[test]
    fn a_new_press_clears_a_stale_report_bit() {
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Sent, 1, false);
        // Bırakma kayboldu, uygulama bu arada kipi kapattı: yeni basış seçim.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT), Drag::Select);
        // Bırakma bayat biti bulup rapor yoluna gitmiyor.
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert!(!gesture.dragging());
    }

    #[test]
    fn a_stale_drag_comes_down() {
        // Seçim sürüklemesinin bırakması kayboldu.
        let mut gesture = Gesture::default();
        press(&mut gesture, Click::Select, 1, false);
        // Kaydırma sistemden tuşun basılı olmadığını öğreniyor.
        gesture.lost_drag();
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
        // Raporlanan yeni basış da bayat `dragging`'i indiriyor.
        press(&mut gesture, Click::Select, 1, false);
        press(&mut gesture, Click::Sent, 1, false);
        assert!(
            !gesture.dragging(),
            "raporlanan basışın yanında bayat seçim"
        );
        assert_eq!(gesture.dragged(LEFT), Drag::Report);
    }

    #[test]
    fn a_dock_press_drags_the_dock_selection() {
        let mut gesture = Gesture::default();
        gesture.begin_press(LEFT);
        assert_eq!(
            gesture.pressed_dock(2, false),
            Press::Select(SelectKind::Word)
        );
        assert_eq!(gesture.dragged(LEFT), Drag::SelectDock);
        // Kaydırma ızgaranın ucunu taşımamalı.
        assert!(!gesture.dragging(), "dock sürüklemesi ızgaranın sayıldı");
        assert_eq!(gesture.released(LEFT), Release::Done);
        assert_eq!(gesture.dragged(LEFT), Drag::Ignore);
        // Shift aynı kuralla uzatma.
        gesture.begin_press(LEFT);
        assert_eq!(gesture.pressed_dock(1, true), Press::Extend);
        // Izgaradaki yeni basış hedefi geri alıyor.
        press(&mut gesture, Click::Select, 1, false);
        assert_eq!(gesture.dragged(LEFT), Drag::Select);
        assert!(gesture.dragging());
    }

    #[test]
    fn motion_is_throttled_to_one_report_per_cell() {
        // Kısmanın tek kuralı: ilk görüşte `true`, aynı hücrenin tekrarında
        // `false`. Bu olmadan işaretçinin her pikseli bir rapor üretir ve
        // boşta duran bir uygulamayı sürekli çizdirirdi.
        let mut gesture = Gesture::default();
        let cell = |col, row, half| SelectionPoint { col, row, half };
        assert!(gesture.moved_to(cell(3, 7, CellHalf::Left)));
        assert!(!gesture.moved_to(cell(3, 7, CellHalf::Left)));
        // **Yarı okunmuyor**: rapor hücre çözünürlüğünde ve hücrenin öteki
        // yarısına geçmek yeni bir rapor doğurmamalı.
        assert!(!gesture.moved_to(cell(3, 7, CellHalf::Right)));
        // Sütun ya da satır değişince rapor yeniden gidiyor.
        assert!(gesture.moved_to(cell(4, 7, CellHalf::Right)));
        assert!(gesture.moved_to(cell(4, 8, CellHalf::Right)));
        // Geri dönüş de bir değişim.
        assert!(gesture.moved_to(cell(4, 7, CellHalf::Right)));
        // Raporlanan basışın damgası aynı hücredeki ilk hareketi yutar.
        gesture.stamp(cell(9, 9, CellHalf::Left));
        assert!(!gesture.moved_to(cell(9, 9, CellHalf::Right)));
    }
}

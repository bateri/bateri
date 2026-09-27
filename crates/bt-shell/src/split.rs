//! Bölme düzeni: sekmenin pane'lerini taşıyan **saf** ikili ağaç (039
//! Karar 6). Yaprak bir pane kimliği (`TerminalPane::id`), düğüm bir eksen ve
//! bir oran. Bölmek, kapatmak ve çerçeve hesabı birer ağaç işlemi; AppKit
//! parçası (`split_view`) yalnız bu çerçeveleri pane'lere uyguluyor ve
//! ayırıcıları boyuyor.
//!
//! AppKit görmüyor, kendi sınamaları var (`quote`/`upload`/`zoom` emsali).
//! Koordinatlar **üstten aşağı** (kapsayıcı `isFlipped`): "aşağı böl"ün
//! ikinci yaprağı altta, yani işaret çevirmek gerekmiyor.
//!
//! Yöne göre komşu, boyutlama, eşitleme ve büyütme phase-4'ün işlemleri ve
//! ilk tüketicileriyle birlikte buraya gelecek.

/// Bölmenin ekseni.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Yan yana — Split Right (⌘D): ikinci yaprak sağda.
    Horizontal,
    /// Üst üste — Split Down (⇧⌘D): ikinci yaprak altta.
    Vertical,
}

/// Dikdörtgen, nokta cinsinden, üstten aşağı.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Rect {
    pub(crate) const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn scaled(self, factor: f64) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.width * factor,
            self.height * factor,
        )
    }
}

/// Ayırıcının kalınlığı, **aygıt pikseli**: bir. Nokta cinsinden `1 / ölçek`
/// — Retina'da yarım nokta. Ölçülmüş değil, bir tasarım sabiti (039 Karar
/// 7: "ayırıcı bir piksel"); dock'un saç çizgileriyle aynı ağırlık.
const DIVIDER_PX: f64 = 1.0;

/// Bir yaprağın çerçevesini `axis`'te ikiye böler: ilk yarı, ayırıcı ve
/// ikinci yarı — pikselde, ayırıcı düşülerek. Oran ilk yarının payı.
///
/// Sınır **tam piksele** oturtuluyor: yarım pikselde duran bir pane'in
/// drawable'ı kesirli olur ve metin bulanıklaşır (026 phase-4'ün sekme
/// çubuğu belirtisi). Girdi zaten tam pikselse çıktının üçü de tam piksel ve
/// girdiyi boşluksuz, örtüşmesiz kaplıyor.
fn halves_px(rect: Rect, axis: Axis, ratio: f64) -> (Rect, Rect, Rect) {
    let span = match axis {
        Axis::Horizontal => rect.width,
        Axis::Vertical => rect.height,
    };
    let available = (span - DIVIDER_PX).max(0.0);
    let first = (available * ratio).round().clamp(0.0, available);
    let second = available - first;
    match axis {
        Axis::Horizontal => (
            Rect::new(rect.x, rect.y, first, rect.height),
            Rect::new(rect.x + first, rect.y, DIVIDER_PX, rect.height),
            Rect::new(rect.x + first + DIVIDER_PX, rect.y, second, rect.height),
        ),
        Axis::Vertical => (
            Rect::new(rect.x, rect.y, rect.width, first),
            Rect::new(rect.x, rect.y + first, rect.width, DIVIDER_PX),
            Rect::new(rect.x, rect.y + first + DIVIDER_PX, rect.width, second),
        ),
    }
}

/// Bir pane bölünse iki yarısının boyu, nokta cinsinden — bölünme sınırının
/// (039 Karar 14) sorusu: çerçeve hesabının **aynı** aritmetiği, yani sınamanın
/// onayladığı yarı çizilecek yarının ta kendisi.
pub(crate) fn split_halves(frame: Rect, axis: Axis, scale: f64) -> (Rect, Rect) {
    let (first, _, second) = halves_px(snap(frame, scale), axis, 0.5);
    (first.scaled(1.0 / scale), second.scaled(1.0 / scale))
}

/// Noktadan piksele, tam piksele yuvarlanmış.
fn snap(rect: Rect, scale: f64) -> Rect {
    let px = rect.scaled(scale);
    Rect::new(
        px.x.round(),
        px.y.round(),
        px.width.round(),
        px.height.round(),
    )
}

/// Bölme ağacı.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tree {
    /// Bir pane'in kimliği.
    Leaf(u64),
    /// İki alt ağaç, `axis`'te yan yana ya da üst üste; `ratio` ilkinin
    /// payı (ayırıcı düşüldükten sonra).
    Split {
        axis: Axis,
        ratio: f64,
        first: Box<Tree>,
        second: Box<Tree>,
    },
}

/// Bir yaprağın kaldırılışının sonucu ([`Tree::remove`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Removal {
    /// Yaprak gitti, kardeşi yukarı çekildi; odak `focus`'a geçmeli.
    Removed { focus: u64 },
    /// Ağacın tek yaprağı: kaldırılmadı — son pane'i kapatmak sekmeyi
    /// kapatmak demek ve o karar çağıranın.
    Last,
    /// Yaprak ağaçta yok.
    Missing,
}

/// Ağacın çerçevelere çevrilmiş hâli ([`Tree::layout`]), nokta cinsinden.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Layout {
    /// Pane kimliği ve çerçevesi, ağaç sırasıyla.
    pub(crate) panes: Vec<(u64, Rect)>,
    /// Ayırıcılar.
    pub(crate) dividers: Vec<Rect>,
}

impl Tree {
    /// Pane kimliklerinin sırası — derinlik öncelikli, ilk alt ağaç önce
    /// (soldan sağa, yukarıdan aşağı).
    pub(crate) fn leaves(&self) -> Vec<u64> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<u64>) {
        match self {
            Tree::Leaf(id) => out.push(*id),
            Tree::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    fn first_leaf(&self) -> u64 {
        match self {
            Tree::Leaf(id) => *id,
            Tree::Split { first, .. } => first.first_leaf(),
        }
    }

    fn last_leaf(&self) -> u64 {
        match self {
            Tree::Leaf(id) => *id,
            Tree::Split { second, .. } => second.last_leaf(),
        }
    }

    /// `target` yaprağını `axis`'te ikiye böler: eski pane ilk yarıda
    /// (solda ya da üstte), `new` ikincide, alan eşit (039 Karar 9).
    /// Yaprak yoksa `false` ve ağaç değişmez.
    pub(crate) fn split(&mut self, target: u64, axis: Axis, new: u64) -> bool {
        match self {
            Tree::Leaf(id) if *id == target => {
                *self = Tree::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Tree::Leaf(target)),
                    second: Box::new(Tree::Leaf(new)),
                };
                true
            }
            Tree::Leaf(_) => false,
            Tree::Split { first, second, .. } => {
                first.split(target, axis, new) || second.split(target, axis, new)
            }
        }
    }

    /// `target` yaprağını kaldırır; kardeşi ebeveynin yerine geçer ve
    /// alanın tamamını alır.
    ///
    /// Odağın gideceği komşu kardeşin **bitişik** yaprağı: kaldırılan ilk
    /// yarıdaysa kardeşin ilk yaprağı, ikincideyse kardeşin son yaprağı —
    /// ikisi de kapanan pane'in ayırıcısına değen pane.
    pub(crate) fn remove(&mut self, target: u64) -> Removal {
        match self {
            Tree::Leaf(id) if *id == target => Removal::Last,
            Tree::Leaf(_) => Removal::Missing,
            Tree::Split { .. } => match remove_in(self, target) {
                Some(focus) => Removal::Removed { focus },
                None => Removal::Missing,
            },
        }
    }

    /// Ağacı `bounds`'a yerleştirir: her pane'in çerçevesi ve ayırıcılar.
    /// `scale` pencerenin ölçeği; sınırlar aygıt pikseline oturuyor
    /// ([`halves_px`]), yani çerçeveler ayırıcılarla birlikte `bounds`'u
    /// boşluksuz ve örtüşmesiz kaplıyor.
    pub(crate) fn layout(&self, bounds: Rect, scale: f64) -> Layout {
        let mut out = Layout::default();
        self.place(snap(bounds, scale), &mut out);
        let points = 1.0 / scale;
        for (_, rect) in &mut out.panes {
            *rect = rect.scaled(points);
        }
        for rect in &mut out.dividers {
            *rect = rect.scaled(points);
        }
        out
    }

    fn place(&self, rect: Rect, out: &mut Layout) {
        match self {
            Tree::Leaf(id) => out.panes.push((*id, rect)),
            Tree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, divider, b) = halves_px(rect, *axis, *ratio);
                first.place(a, out);
                out.dividers.push(divider);
                second.place(b, out);
            }
        }
    }
}

/// [`Tree::remove`]'un düğüm yarısı: `node`'un çocuklarından biri `target`
/// yaprağıysa kardeşi `node`'un yerine koyar ve odağın komşusunu döndürür.
fn remove_in(node: &mut Tree, target: u64) -> Option<u64> {
    let Tree::Split { first, second, .. } = node else {
        return None;
    };
    let (kept, focus) = if **first == Tree::Leaf(target) {
        let focus = second.first_leaf();
        (std::mem::replace(&mut **second, Tree::Leaf(target)), focus)
    } else if **second == Tree::Leaf(target) {
        let focus = first.last_leaf();
        (std::mem::replace(&mut **first, Tree::Leaf(target)), focus)
    } else {
        return remove_in(first, target).or_else(|| remove_in(second, target));
    };
    *node = kept;
    Some(focus)
}

#[cfg(test)]
mod tests {
    use super::{Axis, Rect, Removal, Tree, split_halves};

    fn area(rect: &Rect) -> f64 {
        rect.width * rect.height
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    /// Üç pane: sol yarı, sağ yarı ikiye bölünmüş (L).
    fn three() -> Tree {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(2, Axis::Vertical, 3));
        tree
    }

    #[test]
    fn a_split_gives_two_equal_leaves() {
        let mut tree = Tree::Leaf(7);
        assert!(tree.split(7, Axis::Horizontal, 8));
        assert_eq!(tree.leaves(), vec![7, 8], "eski pane solda, yeni sağda");
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 600.0), 1.0);
        let [(_, left), (_, right)] = layout.panes.as_slice() else {
            panic!("iki çerçeve bekleniyordu: {layout:?}");
        };
        assert_eq!(left.width, right.width, "alan eşit bölünür");
        assert_eq!(left.height, 600.0);
        assert_eq!(layout.dividers, vec![Rect::new(400.0, 0.0, 1.0, 600.0)]);
    }

    #[test]
    fn a_split_down_puts_the_new_pane_below() {
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Vertical, 2));
        let layout = tree.layout(Rect::new(0.0, 0.0, 800.0, 601.0), 1.0);
        let top = layout.panes[0].1;
        let bottom = layout.panes[1].1;
        assert_eq!(layout.panes[1].0, 2);
        assert!(
            bottom.y > top.y,
            "üstten aşağı koordinatta ikinci yaprak altta"
        );
        assert_eq!(top.height, bottom.height);
    }

    #[test]
    fn splitting_a_missing_leaf_changes_nothing() {
        let mut tree = three();
        let before = tree.clone();
        assert!(!tree.split(9, Axis::Horizontal, 10));
        assert_eq!(tree, before);
    }

    #[test]
    fn removing_pulls_the_sibling_up() {
        let mut tree = three();
        // 3'ü kaldır: 2 sağ yarının tamamını alır, ağaç tek bölmeye iner.
        assert_eq!(tree.remove(3), Removal::Removed { focus: 2 });
        let mut expected = Tree::Leaf(1);
        assert!(expected.split(1, Axis::Horizontal, 2));
        assert_eq!(tree, expected);
        // 1'i kaldır: kalan tek yaprak 2.
        assert_eq!(tree.remove(1), Removal::Removed { focus: 2 });
        assert_eq!(tree, Tree::Leaf(2));
    }

    #[test]
    fn the_neighbour_touches_the_closed_pane() {
        // Sol pane (1) kapanınca odak sağ alt ağacın bitişik yaprağına:
        // ilk yaprağı (2, sağ üst).
        let mut tree = three();
        assert_eq!(tree.remove(1), Removal::Removed { focus: 2 });
        // Sağ üst (2) kapanınca kardeşi 3 — tek yaprak, kendisi.
        let mut tree = three();
        assert_eq!(tree.remove(2), Removal::Removed { focus: 3 });
        // İkinci yarının alt ağacı kapanınca odak ilk yarının son yaprağına.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Vertical, 2));
        assert!(tree.split(1, Axis::Horizontal, 3));
        // Düzen: üstte [1 | 3], altta 2. 2 kapanınca odak 3 (üstün son yaprağı).
        assert_eq!(tree.remove(2), Removal::Removed { focus: 3 });
    }

    #[test]
    fn the_last_leaf_is_not_removed() {
        let mut tree = Tree::Leaf(4);
        assert_eq!(tree.remove(4), Removal::Last);
        assert_eq!(tree, Tree::Leaf(4));
        assert_eq!(tree.remove(5), Removal::Missing);
        assert_eq!(three().remove(9), Removal::Missing);
    }

    #[test]
    fn order_is_depth_first() {
        assert_eq!(three().leaves(), vec![1, 2, 3]);
        let mut tree = three();
        assert!(tree.split(1, Axis::Vertical, 4));
        assert_eq!(tree.leaves(), vec![1, 4, 2, 3]);
    }

    #[test]
    fn frames_and_dividers_tile_the_bounds_exactly() {
        // Tek genişlik, iki ölçek ve kesirli nokta sınırı: çerçeveler ile
        // ayırıcılar alanı boşluksuz kaplıyor, örtüşmüyor ve her kenar
        // aygıt pikselinde.
        for scale in [1.0, 2.0] {
            let bounds = Rect::new(0.0, 0.0, 901.5, 603.0);
            let mut tree = three();
            assert!(tree.split(1, Axis::Vertical, 4));
            let layout = tree.layout(bounds, scale);
            let mut rects: Vec<Rect> = layout.panes.iter().map(|(_, rect)| *rect).collect();
            rects.extend(layout.dividers.iter().copied());
            let total: f64 = rects.iter().map(area).sum();
            // Kesirli nokta sınırı 1×'te piksele iniyor (901.5 → 902):
            // kaplanan alan oturtulmuş sınırınki.
            let snapped =
                (bounds.width * scale).round() * (bounds.height * scale).round() / (scale * scale);
            assert!(
                (total - snapped).abs() < 1e-9,
                "ölçek {scale}: toplam {total}"
            );
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(!overlaps(a, b), "ölçek {scale}: {a:?} ∩ {b:?}");
                }
                for edge in [a.x, a.y, a.x + a.width, a.y + a.height] {
                    let px = edge * scale;
                    assert!(
                        (px - px.round()).abs() < 1e-9,
                        "ölçek {scale}: {edge} pikselde değil"
                    );
                }
            }
            for divider in &layout.dividers {
                assert!(
                    (divider.width.min(divider.height) * scale - 1.0).abs() < 1e-9,
                    "ayırıcı bir piksel: {divider:?}"
                );
            }
        }
    }

    #[test]
    fn split_halves_match_the_drawn_frames() {
        // Bölünme sınırının sorduğu yarılar çizilecek çerçevelerin aynısı.
        let bounds = Rect::new(0.0, 0.0, 700.5, 400.0);
        let (first, second) = split_halves(bounds, Axis::Horizontal, 2.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        let layout = tree.layout(bounds, 2.0);
        assert_eq!(layout.panes[0].1, first);
        assert_eq!(layout.panes[1].1, second);
    }
}

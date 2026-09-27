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
//! Gezinme (sıra ve yön), boyutlama (klavye adımı ve ayırıcı sürüklemesi),
//! eşitleme ve büyütme de birer ağaç işlemi (039 phase-4). **En küçük pane**
//! (Karar 14) ağaca yaprak başına bir boyut olarak veriliyor (`min`): punto
//! farkı pane başına, yani hücre de; sınırın kaynağı pane'in kendisi.

/// Bölmenin ekseni.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Yan yana — Split Right (⌘D): ikinci yaprak sağda.
    Horizontal,
    /// Üst üste — Split Down (⇧⌘D): ikinci yaprak altta.
    Vertical,
}

/// Gezinme ve boyutlamanın yönü (⌥⌘ / ⌃⌘ + ok).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Menü öğesinin `tag`'inden yön (`menu`'nün Select/Resize Split ▸
    /// öğeleri); bilinmeyen `tag` `None`.
    pub(crate) fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            2 => Some(Self::Up),
            3 => Some(Self::Down),
            _ => None,
        }
    }

    /// Bu yönün ayırıcısını taşıyan bölmenin ekseni: sola/sağa yan yana
    /// bölmenin, yukarı/aşağı üst üste bölmenin ayırıcısı.
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Up | Self::Down => Axis::Vertical,
        }
    }

    /// Koordinatın büyüdüğü yön mü (sağ, aşağı — üstten aşağı düzende).
    fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

/// Boyut, nokta cinsinden — en küçük pane'in ölçüsü.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Size {
    pub(crate) const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    fn along(self, axis: Axis) -> f64 {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }
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

    /// Eksendeki başlangıç ve uzunluk.
    fn span(self, axis: Axis) -> (f64, f64) {
        match axis {
            Axis::Horizontal => (self.x, self.width),
            Axis::Vertical => (self.y, self.height),
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
    /// Ayırıcılar, ağacın **sıra içi** düzeninde (ilk alt ağacınkiler,
    /// düğümünki, ikincininkiler): [`Tree::drag`]'in indeksi bu sıra.
    pub(crate) dividers: Vec<Divider>,
}

/// Bir ayırıcı: çizgisi ve ayırdığı bölmenin ekseni (yan yana bölmenin
/// ayırıcısı dikey bir çizgi, sürüklemesi yatay).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Divider {
    pub(crate) rect: Rect,
    pub(crate) axis: Axis,
}

/// Kayan nokta karşılaştırmasının payı: çerçeveler aygıt pikseline oturmuş
/// noktalar, yani eşitlik bir yuvarlama gürültüsünden ibaret.
const EPSILON: f64 = 1e-6;

/// `[a, a + a_len)` ile `[b, b + b_len)`'in örtüşen uzunluğu (negatifse sıfır).
fn overlap(a: f64, a_len: f64, b: f64, b_len: f64) -> f64 {
    ((a + a_len).min(b + b_len) - a.max(b)).max(0.0)
}

impl Layout {
    /// `from`'un `direction`'daki komşusu (⌥⌘ + ok): o kenarın ötesinde,
    /// kenara en yakın ve dik eksende en çok örtüşen pane; eşitlikte ağaç
    /// sırasında önce gelen. Kenarda (ötesinde pane yoksa) ya da `from`
    /// düzende değilse `None`.
    pub(crate) fn neighbour(&self, from: u64, direction: Direction) -> Option<u64> {
        let (_, f) = self.panes.iter().find(|(id, _)| *id == from)?;
        let mut best: Option<(u64, f64, f64)> = None;
        for (id, r) in &self.panes {
            if *id == from {
                continue;
            }
            let (gap, shared) = match direction {
                Direction::Right => (r.x - (f.x + f.width), overlap(f.y, f.height, r.y, r.height)),
                Direction::Left => (f.x - (r.x + r.width), overlap(f.y, f.height, r.y, r.height)),
                Direction::Down => (r.y - (f.y + f.height), overlap(f.x, f.width, r.x, r.width)),
                Direction::Up => (f.y - (r.y + r.height), overlap(f.x, f.width, r.x, r.width)),
            };
            if gap < -EPSILON || shared <= EPSILON {
                continue;
            }
            let better = best.is_none_or(|(_, best_gap, best_shared)| {
                gap < best_gap - EPSILON
                    || ((gap - best_gap).abs() <= EPSILON && shared > best_shared + EPSILON)
            });
            if better {
                best = Some((*id, gap, shared));
            }
        }
        best.map(|(id, _, _)| id)
    }
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

    /// `from`'dan sonraki (`forward`) ya da önceki pane, ağaç sırasında ve
    /// döngüsel (⌘] / ⌘[). Tek pane'de ya da `from` ağaçta değilse `None`.
    pub(crate) fn cycle(&self, from: u64, forward: bool) -> Option<u64> {
        let leaves = self.leaves();
        if leaves.len() < 2 {
            return None;
        }
        let index = leaves.iter().position(|id| *id == from)?;
        let next = if forward {
            (index + 1) % leaves.len()
        } else {
            (index + leaves.len() - 1) % leaves.len()
        };
        Some(leaves[next])
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
        for divider in &mut out.dividers {
            divider.rect = divider.rect.scaled(points);
        }
        out
    }

    /// [`Tree::layout`], büyütülmüş yaprakla (⇧⌘↩): `zoomed` ağaçtaysa
    /// yalnız o, bütün alanı kaplayarak ve ayırıcısız; öteki pane'ler
    /// düzende yok (kapsayıcı onları gizliyor). `None` ya da ağaçta olmayan
    /// yaprak sıradan düzen — geri almak ağacı değiştirmediği için eski
    /// çerçeveler bit bit geri geliyor.
    pub(crate) fn layout_zoomed(&self, bounds: Rect, scale: f64, zoomed: Option<u64>) -> Layout {
        match zoomed {
            Some(id) if self.leaves().contains(&id) => Layout {
                panes: vec![(id, snap(bounds, scale).scaled(1.0 / scale))],
                dividers: Vec::new(),
            },
            _ => self.layout(bounds, scale),
        }
    }

    /// ⌃⌘ + ok: `target`'ın `direction`'ın eksenindeki **en yakın atasının**
    /// ayırıcısını o yöne `step` nokta taşır (Ghostty'nin davranışı: ok
    /// ayırıcının gideceği yön, büyüyen pane'in değil). İki taraf en küçük
    /// pane sınırında kırpılıyor ([`place_divider`]). O eksende atası yoksa,
    /// ayırıcı zaten sınırdaysa ya da yaprak ağaçta değilse `false`.
    pub(crate) fn resize(
        &mut self,
        target: u64,
        direction: Direction,
        step: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let step_px = (step * scale).round();
        let delta = if direction.forward() {
            step_px
        } else {
            -step_px
        };
        let limits = Limits { min, scale };
        matches!(
            resize_in(
                self,
                snap(bounds, scale),
                target,
                direction.axis(),
                delta,
                &limits
            ),
            Found::Done(true)
        )
    }

    /// Ayırıcı sürüklemesi: [`Layout::dividers`]'ın `index`'inci ayırıcısını
    /// `position`'a (kapsayıcının koordinatında, nokta, ayırıcının ekseni
    /// boyunca) taşır, sınırda kırparak. Konum değiştiyse `true`.
    pub(crate) fn drag(
        &mut self,
        index: usize,
        position: f64,
        bounds: Rect,
        scale: f64,
        min: &dyn Fn(u64) -> Size,
    ) -> bool {
        let limits = Limits { min, scale };
        let mut index = index;
        drag_in(
            self,
            snap(bounds, scale),
            &mut index,
            position * scale,
            &limits,
        ) == Some(true)
    }

    /// ⌃⌘=: her düğümün oranı alt ağaçlarının **o eksendeki** pane
    /// sayısından — aynı eksende zincirlenmiş bölmeler çocuklarını sayıyor,
    /// öteki eksendeki bir alt ağaç tek sütun (ya da satır). Sonuç: aynı
    /// eksendeki bütün pane'ler eşit (L düzeninde sol pane yarı genişlik,
    /// üçte bir değil).
    pub(crate) fn equalize(&mut self) {
        if let Tree::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            first.equalize();
            second.equalize();
            let a = first.weight(*axis) as f64;
            let b = second.weight(*axis) as f64;
            *ratio = a / (a + b);
        }
    }

    /// `axis`'te yan yana duran pane sayısı ([`Tree::equalize`]).
    fn weight(&self, axis: Axis) -> usize {
        match self {
            Tree::Split {
                axis: own,
                first,
                second,
                ..
            } if *own == axis => first.weight(axis) + second.weight(axis),
            _ => 1,
        }
    }

    /// Bu alt ağacın `axis`'teki en küçük uzunluğu, piksel: her yaprağı en
    /// küçük pane sınırında tutan uzunluk. İç bölmelerin oranı **sabit**
    /// sayılıyor (boyutlama yalnız bir düğümü oynatıyor), yani aynı eksendeki
    /// bölmede sınır toplam değil, payı küçük tarafın payından.
    fn min_px(&self, axis: Axis, limits: &Limits<'_>) -> f64 {
        match self {
            Tree::Leaf(id) => ((limits.min)(*id).along(axis) * limits.scale).ceil(),
            Tree::Split {
                axis: own,
                ratio,
                first,
                second,
            } => {
                let a = first.min_px(axis, limits);
                let b = second.min_px(axis, limits);
                if *own == axis {
                    let r = ratio.clamp(EPSILON, 1.0 - EPSILON);
                    (a / r).max(b / (1.0 - r)).ceil() + DIVIDER_PX
                } else {
                    a.max(b)
                }
            }
        }
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
                out.dividers.push(Divider {
                    rect: divider,
                    axis: *axis,
                });
                second.place(b, out);
            }
        }
    }
}

/// Boyutlamanın sınırı: yaprak başına en küçük boyut (nokta) ve ölçek.
struct Limits<'a> {
    min: &'a dyn Fn(u64) -> Size,
    scale: f64,
}

/// Bir düğümün ayırıcısını ilk yarının `desired` piksel olacağı yere
/// taşımayı dener: iki taraf da en küçük uzunluğunun altına inmez
/// ([`Tree::min_px`]). İki sınır birbirini geçiyorsa (alan zaten dar)
/// hiçbir şey değişmez. İlk yarının piksel boyu değiştiyse `true`.
fn place_divider(
    axis: Axis,
    ratio: &mut f64,
    first: &Tree,
    second: &Tree,
    rect: Rect,
    desired: f64,
    limits: &Limits<'_>,
) -> bool {
    let (_, span) = rect.span(axis);
    let available = (span - DIVIDER_PX).max(0.0);
    if available <= 0.0 {
        return false;
    }
    let low = first.min_px(axis, limits);
    let high = available - second.min_px(axis, limits);
    if low > high {
        return false;
    }
    let current = (available * *ratio).round().clamp(0.0, available);
    let target = desired.round().clamp(low, high);
    if target == current {
        return false;
    }
    *ratio = target / available;
    true
}

/// [`Tree::resize`]'ın aramasının sonucu.
enum Found {
    /// Yaprak bu alt ağaçta değil.
    Absent,
    /// Yaprak burada ama henüz o eksende bir ata bulunmadı.
    Pending,
    /// Ata bulundu; ayırıcı oynadıysa `true`.
    Done(bool),
}

fn resize_in(
    node: &mut Tree,
    rect: Rect,
    target: u64,
    axis: Axis,
    delta: f64,
    limits: &Limits<'_>,
) -> Found {
    match node {
        Tree::Leaf(id) if *id == target => Found::Pending,
        Tree::Leaf(_) => Found::Absent,
        Tree::Split {
            axis: own,
            ratio,
            first,
            second,
        } => {
            let (a, _, b) = halves_px(rect, *own, *ratio);
            let found = match resize_in(first, a, target, axis, delta, limits) {
                Found::Absent => resize_in(second, b, target, axis, delta, limits),
                found => found,
            };
            match found {
                Found::Pending if *own == axis => {
                    let (_, current) = a.span(axis);
                    Found::Done(place_divider(
                        axis,
                        ratio,
                        first,
                        second,
                        rect,
                        current + delta,
                        limits,
                    ))
                }
                found => found,
            }
        }
    }
}

/// [`Tree::drag`]'in yürüyüşü: ayırıcıları [`Tree::place`]'in sırasıyla
/// sayıyor, `index` sıfıra inen düğümün ayırıcısını `position`'a (piksel)
/// taşıyor. Ayırıcı bulunmadıysa `None`.
fn drag_in(
    node: &mut Tree,
    rect: Rect,
    index: &mut usize,
    position: f64,
    limits: &Limits<'_>,
) -> Option<bool> {
    let Tree::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        return None;
    };
    let (a, _, b) = halves_px(rect, *axis, *ratio);
    if let Some(done) = drag_in(first, a, index, position, limits) {
        return Some(done);
    }
    if *index == 0 {
        let (start, _) = rect.span(*axis);
        return Some(place_divider(
            *axis,
            ratio,
            first,
            second,
            rect,
            position - start,
            limits,
        ));
    }
    *index -= 1;
    drag_in(second, b, index, position, limits)
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
    use super::{Axis, Direction, Divider, Layout, Rect, Removal, Size, Tree, split_halves};

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
        assert_eq!(
            layout.dividers,
            vec![Divider {
                rect: Rect::new(400.0, 0.0, 1.0, 600.0),
                axis: Axis::Horizontal,
            }]
        );
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
            rects.extend(layout.dividers.iter().map(|divider| divider.rect));
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
                let divider = divider.rect;
                assert!(
                    (divider.width.min(divider.height) * scale - 1.0).abs() < 1e-9,
                    "ayırıcı bir piksel: {divider:?}"
                );
            }
        }
    }

    fn frame_of(layout: &Layout, id: u64) -> Rect {
        layout
            .panes
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
            .expect("pane düzende olmalı")
    }

    fn no_min(_: u64) -> Size {
        Size::new(0.0, 0.0)
    }

    #[test]
    fn the_neighbour_is_found_by_direction_in_an_l_layout() {
        // Düzen: sol 1 (tam boy) | sağ üst 2 / sağ alt 3.
        let tree = three();
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 601.0), 1.0);
        assert_eq!(
            layout.neighbour(1, Direction::Right),
            Some(2),
            "eşit örtüşmede ağaç sırası"
        );
        assert_eq!(layout.neighbour(2, Direction::Left), Some(1));
        assert_eq!(layout.neighbour(3, Direction::Left), Some(1));
        assert_eq!(layout.neighbour(2, Direction::Down), Some(3));
        assert_eq!(layout.neighbour(3, Direction::Up), Some(2));
        assert_eq!(
            layout.neighbour(1, Direction::Left),
            None,
            "kenarda komşu yok"
        );
        assert_eq!(layout.neighbour(1, Direction::Up), None);
        assert_eq!(layout.neighbour(2, Direction::Up), None);
        assert_eq!(
            layout.neighbour(9, Direction::Up),
            None,
            "düzende olmayan pane"
        );
        // Sağ alt yukarı taşınınca sol pane en çok 3'le örtüşüyor.
        let mut tree = three();
        assert!(tree.resize(
            3,
            Direction::Up,
            200.0,
            Rect::new(0.0, 0.0, 801.0, 601.0),
            1.0,
            &no_min
        ));
        let layout = tree.layout(Rect::new(0.0, 0.0, 801.0, 601.0), 1.0);
        assert_eq!(
            layout.neighbour(1, Direction::Right),
            Some(3),
            "en çok örtüşen"
        );
    }

    #[test]
    fn next_and_previous_cycle_in_tree_order() {
        let tree = three();
        assert_eq!(tree.cycle(1, true), Some(2));
        assert_eq!(tree.cycle(3, true), Some(1), "sondan başa");
        assert_eq!(tree.cycle(1, false), Some(3), "baştan sona");
        assert_eq!(Tree::Leaf(1).cycle(1, true), None, "tek pane");
        assert_eq!(tree.cycle(9, true), None);
    }

    #[test]
    fn resizing_moves_the_nearest_divider_on_that_axis() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let mut tree = three();
        // 2 sağ üstte: yatay eksendeki en yakın ata kök. Sağa → ayırıcı sağa.
        assert!(tree.resize(2, Direction::Right, 10.0, bounds, 1.0, &no_min));
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, 410.0);
        assert_eq!(frame_of(&layout, 2).width, 390.0);
        assert_eq!(frame_of(&layout, 3).width, 390.0, "aynı alt ağaç birlikte");
        // Dikey eksende en yakın ata sağ alt ağaç; 1'in dikey atası yok.
        assert!(tree.resize(2, Direction::Down, 20.0, bounds, 1.0, &no_min));
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 2).height, 320.0);
        assert_eq!(frame_of(&layout, 3).height, 280.0);
        assert!(!tree.resize(1, Direction::Down, 20.0, bounds, 1.0, &no_min));
        assert!(!tree.resize(9, Direction::Right, 20.0, bounds, 1.0, &no_min));
    }

    #[test]
    fn resizing_stops_at_the_minimum_and_keeps_the_area() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let min = |_: u64| Size::new(200.0, 150.0);
        let mut tree = three();
        let covered = |layout: &Layout| -> f64 {
            layout.panes.iter().map(|(_, r)| area(r)).sum::<f64>()
                + layout.dividers.iter().map(|d| area(&d.rect)).sum::<f64>()
        };
        let area_before = covered(&tree.layout(bounds, 1.0));
        // Sola sürekli: 1 en küçük genişlikte duruyor.
        let mut steps = 0;
        while tree.resize(1, Direction::Left, 50.0, bounds, 1.0, &min) {
            steps += 1;
            assert!(steps < 100, "sınırda durmalı");
        }
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, 200.0);
        let area_after = covered(&layout);
        assert_eq!(area_before, area_after, "toplam alan korunuyor");
        // Sağa sürekli: sağ alt ağaç en küçük genişlikte duruyor.
        while tree.resize(1, Direction::Right, 50.0, bounds, 1.0, &min) {}
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 2).width, 200.0);
        assert_eq!(frame_of(&layout, 3).width, 200.0);
        // Aşağı: 3 en küçük boyda.
        while tree.resize(2, Direction::Down, 50.0, bounds, 1.0, &min) {}
        assert_eq!(frame_of(&tree.layout(bounds, 1.0), 3).height, 150.0);
    }

    #[test]
    fn a_nested_side_is_limited_by_its_smallest_pane() {
        // Sol [1 | 4] (4 dar ama sınırın üstünde), sağ 2. Sol alt ağacı daraltmak 4'ü de
        // daraltıyor: sınır toplam değil, 4'ün payından.
        let bounds = Rect::new(0.0, 0.0, 1001.0, 400.0);
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(1, Axis::Horizontal, 4));
        let layout = tree.layout(bounds, 1.0);
        let divider = layout.dividers[0];
        assert!(tree.drag(0, divider.rect.x + 140.0, bounds, 1.0, &no_min));
        assert!(frame_of(&tree.layout(bounds, 1.0), 4).width > 100.0);
        let min = |_: u64| Size::new(100.0, 10.0);
        while tree.resize(2, Direction::Left, 25.0, bounds, 1.0, &min) {}
        let layout = tree.layout(bounds, 1.0);
        for id in [1, 2, 4] {
            assert!(frame_of(&layout, id).width >= 100.0, "{id}: {layout:?}");
        }
    }

    #[test]
    fn a_drag_moves_exactly_the_dragged_divider() {
        let bounds = Rect::new(0.0, 0.0, 801.0, 601.0);
        let mut tree = three();
        assert!(tree.split(1, Axis::Vertical, 4));
        let before = tree.layout(bounds, 1.0);
        for (index, divider) in before.dividers.iter().enumerate() {
            let mut moved = tree.clone();
            let (position, axis) = match divider.axis {
                Axis::Horizontal => (divider.rect.x - 30.0, Axis::Horizontal),
                Axis::Vertical => (divider.rect.y - 30.0, Axis::Vertical),
            };
            assert!(moved.drag(index, position, bounds, 1.0, &no_min));
            let after = moved.layout(bounds, 1.0);
            for (other, (a, b)) in before.dividers.iter().zip(&after.dividers).enumerate() {
                if other == index {
                    let (was, now) = match axis {
                        Axis::Horizontal => (a.rect.x, b.rect.x),
                        Axis::Vertical => (a.rect.y, b.rect.y),
                    };
                    assert_eq!(now, was - 30.0, "sürüklenen ayırıcı {index} işaretçide");
                } else if a.axis == b.axis && a.axis != axis {
                    // Öteki eksendeki ayırıcıların konumu kaymaz (boyu değişebilir).
                    let (was, now) = match a.axis {
                        Axis::Horizontal => (a.rect.x, b.rect.x),
                        Axis::Vertical => (a.rect.y, b.rect.y),
                    };
                    assert_eq!(was, now, "ayırıcı {other} yerinde");
                }
            }
        }
        // Sınırın dışına sürükleme kırpılıyor, olmayan ayırıcı no-op.
        let min = |_: u64| Size::new(100.0, 100.0);
        let mut tree = three();
        assert!(tree.drag(0, -500.0, bounds, 1.0, &min));
        assert_eq!(frame_of(&tree.layout(bounds, 1.0), 1).width, 100.0);
        assert!(!tree.drag(7, 10.0, bounds, 1.0, &min));
    }

    #[test]
    fn equalizing_gives_panes_on_one_axis_the_same_span() {
        let bounds = Rect::new(0.0, 0.0, 901.0, 601.0);
        // Üç sütun [1 | 2 | 3] (iç içe) ve sağ sütun ikiye bölünmüş: sütunlar
        // eşit, sağ sütunun iki pane'i eşit.
        let mut tree = Tree::Leaf(1);
        assert!(tree.split(1, Axis::Horizontal, 2));
        assert!(tree.split(2, Axis::Horizontal, 3));
        assert!(tree.split(3, Axis::Vertical, 4));
        assert!(tree.drag(0, 100.0, bounds, 1.0, &no_min));
        assert!(tree.drag(2, 150.0, bounds, 1.0, &no_min));
        tree.equalize();
        let layout = tree.layout(bounds, 1.0);
        let widths: Vec<f64> = [1, 2, 3]
            .iter()
            .map(|id| frame_of(&layout, *id).width)
            .collect();
        for width in &widths {
            assert!(
                (width - widths[0]).abs() <= 1.0,
                "sütunlar eşit: {widths:?}"
            );
        }
        assert!((frame_of(&layout, 3).height - frame_of(&layout, 4).height).abs() <= 1.0);
        // L düzeninde sol pane yarı genişlik: yaprak sayısı değil eksendeki sayı.
        let mut tree = three();
        assert!(tree.drag(0, 100.0, bounds, 1.0, &no_min));
        tree.equalize();
        let layout = tree.layout(bounds, 1.0);
        assert_eq!(frame_of(&layout, 1).width, frame_of(&layout, 2).width);
    }

    #[test]
    fn a_zoomed_leaf_takes_the_whole_area_and_gives_it_back() {
        let bounds = Rect::new(0.0, 0.0, 801.5, 601.0);
        let tree = three();
        let before = tree.layout(bounds, 2.0);
        let zoomed = tree.layout_zoomed(bounds, 2.0, Some(3));
        assert_eq!(zoomed.panes, vec![(3, Rect::new(0.0, 0.0, 801.5, 601.0))]);
        assert!(zoomed.dividers.is_empty(), "büyütülmüşken ayırıcı yok");
        assert_eq!(
            tree.layout_zoomed(bounds, 2.0, None),
            before,
            "geri alınınca eski çerçeveler"
        );
        assert_eq!(
            tree.layout_zoomed(bounds, 2.0, Some(9)),
            before,
            "ağaçta olmayan yaprak"
        );
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

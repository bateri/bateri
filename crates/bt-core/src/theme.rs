//! Tema dosyası: `themes/{ad}.toml`'un metninden [`Theme`]'e giden saf yol.
//!
//! Dosya sistemi **görmez**: adı dosyaya ya da gömülü temaya çözen ve metni
//! okuyan `bt-shell`. Biçimin kaydı `.tasks/007-ayarlar-ve-tema/discussion.md`
//! → Karar 3, kullanıcıya anlatılışı `docs/AYARLAR.md` → Temalar.
//!
//! **Taban üstüne okunur:** her anahtar opsiyonel, eksik anahtar tabandan
//! gelir — kullanıcı gömülü bir temayı kopyalayıp yalnız değiştirdiğini
//! bırakabilir. Hata kuralı ayar dosyasınınkiyle aynı: ayrıştırılamayan metin
//! ayrı sonuç (`Err`), ayrıştırılan metinde kabul edilmeyen renk tabandaki
//! değeri alır ve tanı bırakır, bilinmeyen anahtar sessiz — 013'ün kalan durum
//! rolleri (uyarı, bilgi) bugünkü sürümde hata sayılmamalı.

use toml_edit::TableLike;

use crate::color::Theme;
use crate::settings::{Diagnostic, document, kind, line_of, section};

/// `[ansi]` bölümünün anahtarları, [`Theme::ansi`] sırasıyla; ikincisi tanıdaki
/// noktalı yol (`Diagnostic::key` `'static` ister).
const ANSI_KEYS: [(&str, &str); 16] = [
    ("black", "ansi.black"),
    ("red", "ansi.red"),
    ("green", "ansi.green"),
    ("yellow", "ansi.yellow"),
    ("blue", "ansi.blue"),
    ("magenta", "ansi.magenta"),
    ("cyan", "ansi.cyan"),
    ("white", "ansi.white"),
    ("bright_black", "ansi.bright_black"),
    ("bright_red", "ansi.bright_red"),
    ("bright_green", "ansi.bright_green"),
    ("bright_yellow", "ansi.bright_yellow"),
    ("bright_blue", "ansi.bright_blue"),
    ("bright_magenta", "ansi.bright_magenta"),
    ("bright_cyan", "ansi.bright_cyan"),
    ("bright_white", "ansi.bright_white"),
];

impl Theme {
    /// Tema dosyasının metni → `base`'in üstüne okunmuş tema + tanılar, ya da
    /// ayrıştırılamadı.
    ///
    /// Roller (`background`, `foreground`, `dim`, `accent`, `cursor`,
    /// `success`, `error`) kökte, 16 renk
    /// `[ansi]` bölümünde; renk `"#rrggbb"` (büyük harf de olur). `Err` yalnız
    /// geçersiz TOML'da, ayar dosyasındaki anlamıyla.
    ///
    /// Taban parametre, sabit değil: üretimde hep `Theme::BATERI` (eksik
    /// anahtarın nereden geleceği `bt-shell`'in kararı), ama belgedeki tema
    /// bloğunun sınaması **her değeri ayrık** bir tabanla okuyor ki eksik bir
    /// anahtar tabandan sessizce dolmasın.
    pub fn parse(text: &str, base: &Theme) -> Result<(Theme, Vec<Diagnostic>), Diagnostic> {
        let doc = document(text)?;
        let root = doc.as_table();
        let mut theme = *base;
        let mut diagnostics = Vec::new();
        let roles = [
            ("background", &mut theme.background),
            ("foreground", &mut theme.foreground),
            ("dim", &mut theme.dim),
            ("accent", &mut theme.accent),
            ("cursor", &mut theme.cursor),
            ("success", &mut theme.success),
            ("error", &mut theme.error),
        ];
        for (key, slot) in roles {
            read_color(text, root, key, key, slot, &mut diagnostics);
        }
        if let Some(ansi) = section(text, root, "ansi", &mut diagnostics) {
            for ((key, path), slot) in ANSI_KEYS.into_iter().zip(&mut theme.ansi) {
                read_color(text, ansi, key, path, slot, &mut diagnostics);
            }
        }
        Ok((theme, diagnostics))
    }
}

/// Bir renk anahtarını okur; yoksa yuvaya dokunmaz, kabul edilmezse tanı
/// bırakır ve yuvadaki taban değeri kalır.
fn read_color(
    text: &str,
    table: &dyn TableLike,
    key: &str,
    path: &'static str,
    slot: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(item) = table.get(key) else {
        return;
    };
    let found = match item.as_str() {
        Some(value) => match hex_color(value) {
            Some(color) => {
                *slot = color;
                return;
            }
            None => format!("{value:?}"),
        },
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(path),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{path}` must be a color like \"#rrggbb\", found {found}; using #{:06x}",
            *slot
        ),
    });
}

/// `"#rrggbb"` → `0xRRGGBB`. Kısa (`#rgb`) ve alfalı (`#rrggbbaa`) biçim yok:
/// tek biçim, tek tanı.
fn hex_color(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#')?;
    // `from_str_radix` baştaki `+`'yı kabul ediyor; önce altı hane mi bak.
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Her değeri `Theme::BATERI`'den ve birbirinden ayrık bir taban: eksik bir
    /// anahtar okunduğunda tabandan gelen değer hiçbir gerçek renkle
    /// karışmasın.
    const SENTINEL: Theme = Theme {
        background: 0x000001,
        foreground: 0x000002,
        dim: 0x000003,
        accent: 0x000004,
        cursor: 0x000007,
        success: 0x000005,
        error: 0x000006,
        ansi: [
            0x000010, 0x000011, 0x000012, 0x000013, 0x000014, 0x000015, 0x000016, 0x000017,
            0x000018, 0x000019, 0x00001a, 0x00001b, 0x00001c, 0x00001d, 0x00001e, 0x00001f,
        ],
    };

    fn clean(text: &str, base: &Theme) -> Theme {
        let (theme, diagnostics) = Theme::parse(text, base).expect("ayrıştırılabilir metin");
        assert_eq!(diagnostics, Vec::new(), "tanı beklenmiyordu: {text}");
        theme
    }

    #[test]
    fn empty_theme_is_the_base() {
        assert_eq!(clean("", &SENTINEL), SENTINEL);
        assert_eq!(clean("# yalnız yorum\n", &Theme::BATERI), Theme::BATERI);
    }

    #[test]
    fn the_cursor_role_reads_like_every_other() {
        // **Tek kural, istisnasız** (kullanıcı kararı, 2026-09-19). `cursor`
        // bir dönem eksikte `accent`'e düşüyordu; gerekçesi "rolden önce
        // yazılmış tema dosyaları değişmesin"di, ama uygulama yayınlanmadığı
        // için koruduğu kimse yoktu ve 20+ anahtar içinde **tek** istisnaydı.
        let theme = clean("accent = \"#ff0000\"\ncursor = \"#00ff00\"\n", &SENTINEL);
        assert_eq!((theme.accent, theme.cursor), (0xff0000, 0x00ff00));
        // Yalnız `accent` yazmak imleci **etkilemiyor**: o tabandan geliyor.
        let theme = clean("accent = \"#ff0000\"\n", &SENTINEL);
        assert_eq!((theme.accent, theme.cursor), (0xff0000, SENTINEL.cursor));
        // Kabul edilmeyen değer yuvayı tabanda bırakıyor ve tanı bırakıyor.
        let (theme, diagnostics) =
            Theme::parse("cursor = \"yeşil\"\n", &SENTINEL).expect("ayrıştırılabilir metin");
        assert_eq!(theme.cursor, SENTINEL.cursor);
        assert_eq!(diagnostics.len(), 1, "tanı yok: {diagnostics:?}");
    }

    #[test]
    fn partial_theme_fills_from_the_base() {
        let theme = clean(
            "background = \"#FFFFFF\"\naccent = \"#ff0000\"\n[ansi]\nbright_white = \"#010203\"\n",
            &Theme::BATERI,
        );
        assert_eq!(
            theme,
            Theme {
                background: 0xffffff,
                accent: 0xff0000,
                ansi: {
                    let mut ansi = Theme::BATERI.ansi;
                    ansi[15] = 0x010203;
                    ansi
                },
                ..Theme::BATERI
            }
        );
    }

    #[test]
    fn ansi_keys_follow_the_palette_order() {
        let text = ANSI_KEYS
            .iter()
            .enumerate()
            .map(|(i, (key, _))| format!("{key} = \"#0000{:02x}\"\n", 0xa0 + i))
            .collect::<String>();
        let theme = clean(&format!("[ansi]\n{text}"), &SENTINEL);
        let expected: Vec<u32> = (0..16).map(|i| 0xa0 + i).collect();
        assert_eq!(theme.ansi.to_vec(), expected);
        // Adlar da sırayla: 1 kırmızı, 9 parlak kırmızı.
        assert_eq!((ANSI_KEYS[1].0, ANSI_KEYS[9].0), ("red", "bright_red"));
    }

    #[test]
    fn bad_color_keeps_the_base_value_with_diagnostic() {
        let (theme, diagnostics) = Theme::parse(
            "background = \"#12345\"\n[ansi]\nred = 16711680\ngreen = \"#+12345\"\n",
            &Theme::BATERI,
        )
        .expect("ayrıştırılabilir metin");
        assert_eq!(theme, Theme::BATERI);
        let lines: Vec<_> = diagnostics.iter().map(ToString::to_string).collect();
        assert_eq!(
            lines,
            [
                "line 1: `background` must be a color like \"#rrggbb\", found \"#12345\"; using #000000",
                "line 3: `ansi.red` must be a color like \"#rrggbb\", found an integer; using #d16d6a",
                "line 4: `ansi.green` must be a color like \"#rrggbb\", found \"#+12345\"; using #8bb58b",
            ]
        );
        assert_eq!(diagnostics[1].key, Some("ansi.red"));
    }

    #[test]
    fn ansi_of_wrong_type_is_diagnosed() {
        let (theme, diagnostics) =
            Theme::parse("ansi = \"#ffffff\"\n", &Theme::BATERI).expect("ayrıştırılabilir metin");
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].key, Some("ansi"));
    }

    #[test]
    fn unknown_keys_are_silent() {
        // 013'ün **kalan** durum rolleri ve başka terminallerin ek anahtarları.
        // Sentinel 010'da değişti: `success` artık bilinen bir anahtar ve
        // sınama onu örnek olarak kullansaydı sessizce hiçbir şey sormaz olurdu.
        let theme = clean(
            "name = \"x\"\nwarning = \"#00ff00\"\n[ansi]\nred = \"#ff0000\"\norange = 1\n[meta]\n",
            &Theme::BATERI,
        );
        assert_eq!(theme.ansi[1], 0xff0000);
    }

    #[test]
    fn status_roles_are_read_and_inherited() {
        // Yazılan rol okunuyor…
        let theme = clean("success = \"#0a0b0c\"\n", &SENTINEL);
        assert_eq!(
            theme,
            Theme {
                success: 0x0a0b0c,
                ..SENTINEL
            }
        );
        // …yazılmayan rol tabandan geliyor. 010'un göç cümlesi bu: kendi
        // temasını yazmış kullanıcı iki rolü gömülü `bateri`'den miras alır.
        let inherited = clean("background = \"#ffffff\"\n", &Theme::BATERI);
        assert_eq!(
            (inherited.success, inherited.error),
            (Theme::BATERI.success, Theme::BATERI.error)
        );
    }

    #[test]
    fn unparseable_theme_is_a_separate_result() {
        let err =
            Theme::parse("background = \"#ffffff\n", &Theme::BATERI).expect_err("geçersiz TOML");
        assert_eq!(err.line, Some(1));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");
    }

    #[test]
    fn documented_blocks_are_the_embedded_themes() {
        // `docs/AYARLAR.md` "kopyala, değiştir" diye her gömülü tema için tam
        // bir blok veriyor. Belge değer **kopyaladığı** için drift eder; bu
        // sınama blokları gömülü temalara bağlıyor. Taban ayrık: bloktan düşen
        // bir anahtar tabandan dolup eşitliği bozar, yani blok eksiksiz kalmak
        // zorunda. Liste tablonun kendisinden: eklenen gömülü tema bloğuyla
        // gelmek zorunda.
        let doc = include_str!("../../../docs/AYARLAR.md");
        let names: Vec<_> = Theme::embedded_names().collect();
        assert_eq!(names, ["bateri", "bateri-light"]);
        for name in names {
            let theme = Theme::embedded(name);
            // Başlık satır sonuyla aranıyor: "`bateri`" "`bateri-light`"in
            // öneki ve sonsuz arama yanlış bloğu okurdu.
            let heading = format!("### Gömülü `{name}`\n");
            let (_, after) = doc
                .split_once(&heading)
                .unwrap_or_else(|| panic!("AYARLAR.md'de {heading:?} yok"));
            let (_, block) = after
                .split_once("```toml\n")
                .expect("başlığın altında toml bloğu yok");
            let (block, _) = block.split_once("```").expect("toml bloğu kapanmıyor");
            assert_eq!(Some(clean(block, &SENTINEL)), theme, "{name}");
        }
    }
}

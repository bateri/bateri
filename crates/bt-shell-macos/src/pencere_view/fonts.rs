//! The app's own fonts, for the page of a program's view.
//!
//! The web engine draws a page in a process of its own, and the fonts the app registered
//! for itself at launch (`bt_gpu::register_app_fonts`) belong to this process alone: a
//! family the app ships and the machine does not have would be the system's monospace in
//! the page while it is the chosen font in the terminal's grid. So the page is **given the
//! files**, from its own origin, and told which face each is:
//!
//! - a file is served at `/bateri-font:{name}` of the view's origin ([`ROUTE`]). A path in a
//!   bundle's manifest never holds a colon (the protocol's grammar of paths is closed), so
//!   the name cannot be a file of any bundle, nor an attached file (`sha256:…` is a digest,
//!   not this prefix), and the view's policy (`font-src 'self'`) lets the page load it;
//! - a user script, which runs at the start of every document of the web view, declares the
//!   faces with `document.fonts.add(new FontFace(…))` ([`script`]) and does nothing in any
//!   document that is not the view's own: the shell page, and a frame the page nested.
//!
//! The **family is the file's own** and so are the style and the weights (`bt-atlas` reads
//! them from the file when it registers it), and only the files that were registered are
//! here: a family the user has installed was left alone, and a `FontFace` of the same name
//! would have covered their copy. Every family of the app is declared, not only the chosen
//! one: a face is fetched when the page uses it, and a family chosen later needs nothing new.
//!
//! The symbol font is not given to the page (the grid draws from it the private-use symbols
//! a font lacks; a page has no use for it), and no italic face is made up: a family with no
//! italic file (Fira Code) is slanted by the engine, where the grid draws it upright.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_gpu::AppFace;

use super::serve::{SCHEME, Served, js_string};

/// What a request path for one of the app's fonts starts with, after the `/`.
pub(super) const ROUTE: &str = "bateri-font:";

/// One face the page is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Face {
    /// The family, as the file declares it and as the page names it.
    pub(super) family: String,
    /// What the page asks for the file by: the file's own name.
    pub(super) name: String,
    /// Where the file is.
    pub(super) file: PathBuf,
    pub(super) italic: bool,
    /// The weights it covers, lowest and highest.
    pub(super) weight: (u16, u16),
}

/// The faces the page of a view is given: those of the files `bt-atlas` registered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Fonts {
    faces: Vec<Face>,
}

/// Whether `name` is something the route can name: one name, never a path.
pub(super) fn is_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', '\0'])
}

/// The media type of the font file called `name`, by what it ends in; `None` for what is not
/// one of the two formats a page is given.
fn media_of(name: &str) -> Option<&'static str> {
    let extension = Path::new(name).extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("ttf") {
        Some("font/ttf")
    } else if extension.eq_ignore_ascii_case("otf") {
        Some("font/otf")
    } else {
        None
    }
}

/// Words that name no family when they stand alone as one: the generic families and the keywords
/// every CSS value takes. A family called one of them would be read as the keyword.
const KEYWORDS: [&str; 14] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-monospace",
    "ui-serif",
    "ui-sans-serif",
    "ui-rounded",
    "inherit",
    "initial",
    "unset",
    "default",
];

/// Whether `family` is a name `FontFace` takes as it stands: words of letters, digits, hyphens
/// and underscores, each starting with a letter, one space between them (anything else is
/// escaped or collapsed when it is read as a CSS name, and the page would name another family).
fn writable(family: &str) -> bool {
    let words: Vec<&str> = family.split(' ').collect();
    let word = |word: &&str| {
        let mut chars = word.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    };
    words.iter().all(word)
        && !(words.len() == 1 && KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(family)))
}

impl Fonts {
    /// The faces of `registered`. A file whose name is not one the route can name, whose
    /// format is not a page's, whose family cannot be written, or whose name an earlier file
    /// already has (the earlier one is what the name serves) is left out; so is the symbol
    /// font, which `bt-atlas` does not list.
    pub(super) fn of(registered: &[AppFace]) -> Self {
        let mut faces: Vec<Face> = Vec::new();
        for face in registered {
            let Some(name) = face.file.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_name(name)
                || media_of(name).is_none()
                || !writable(&face.family)
                || faces.iter().any(|known| known.name == name)
            {
                continue;
            }
            faces.push(Face {
                family: face.family.clone(),
                name: name.to_owned(),
                file: face.file.clone(),
                italic: face.italic,
                weight: face.weight,
            });
        }
        Self { faces }
    }

    /// The faces of the fonts this process registered at launch: none in a process that never
    /// ran the pass (a test, a build without the directory).
    pub(super) fn registered() -> Self {
        Self::of(bt_gpu::app_fonts().map_or(&[][..], |fonts| &fonts.faces))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The faces, in the order they were registered.
    #[cfg(test)]
    pub(super) fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// The bytes and media type of the font called `name`, exactly as the list names it: the
    /// answer to a request for [`ROUTE`] and the name. The name is matched against the list and
    /// never joined to a directory, so nothing outside the list is reachable by it. **Reads the
    /// disk**: the caller is the file queue's.
    pub(super) fn serve(&self, name: &str) -> Option<Served> {
        let face = self.faces.iter().find(|face| face.name == name)?;
        let media = media_of(&face.name)?;
        let bytes = std::fs::read(&face.file).ok()?;
        Some((media.to_owned(), Arc::from(bytes)))
    }

    /// The user script that declares the faces in the view of `instance`, or `None` when
    /// there is no face to declare.
    pub(super) fn script(&self, instance: &str) -> Option<String> {
        (!self.is_empty()).then(|| script(instance, &self.faces))
    }
}

/// The text of the user script: declares `faces` in a document of the view `instance`'s origin
/// and in no other. The web view runs it in every frame (the shell's, the view's and any the
/// page nests), so the first thing it does is look at where it is: the scheme and the host of
/// the document are the view's own, which no other frame of the web view has (the shell page's
/// host is another, a frame the page nests is not of this scheme). `location.origin` is not
/// asked, since what an origin says of a scheme the engine does not know is not the same
/// everywhere.
pub(super) fn script(instance: &str, faces: &[Face]) -> String {
    let list: Vec<String> = faces
        .iter()
        .map(|face| {
            let (low, high) = face.weight;
            let weight = if low == high {
                low.to_string()
            } else {
                format!("{low} {high}")
            };
            let style = if face.italic { "italic" } else { "normal" };
            let family = face.family.as_str();
            // The path is `/…` of the document's own origin: a bare `bateri-font:…` would be read
            // as a URL of a scheme of that name.
            let path = format!("/{ROUTE}{}", super::serve::percent_encode_path(&face.name));
            format!(
                "[{}, {}, {}, {}]",
                js_string(family),
                js_string(&path),
                js_string(&weight),
                js_string(style)
            )
        })
        .collect();
    format!(
        "(function () {{\n\
         if (location.protocol !== {protocol} || location.hostname.toLowerCase() !== {host}) return;\n\
         if (typeof FontFace !== \"function\" || !document.fonts) return;\n\
         var faces = [{faces}];\n\
         faces.forEach(function (face) {{\n\
         try {{\n\
         document.fonts.add(new FontFace(face[0], \"url(\" + face[1] + \")\", {{ weight: face[2], style: face[3] }}));\n\
         }} catch (error) {{}}\n\
         }});\n\
         }})();\n",
        protocol = js_string(&format!("{SCHEME}:")),
        host = js_string(&instance.to_ascii_lowercase()),
        faces = list.join(", "),
    )
}

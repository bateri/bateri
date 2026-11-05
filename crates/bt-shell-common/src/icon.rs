//! The drawing that stands for a program: what a tab's icon is made of.
//!
//! [`crate::program::identity`] says what a pane runs ([`Kind`]); this module
//! says what that looks like, as data a platform shell turns into its own
//! drawing calls and a different shell could turn into another's. No toolkit is
//! named here: an icon is the path data its source publishes (read by
//! [`parse`] into moves, lines, cubics and closes, placed in a slot by
//! [`ViewBox::fit`]) or, for the one mark that exists only as a picture, an
//! alpha mask.
//!
//! Two families. The **agents'** marks are their own brand's, one-colour and
//! filled, so the agent is known at a glance; four of them have a colour of
//! their own ([`Icon::brand`]), for a setting that asks for it. The **kinds**
//! (a shell, a server, an editor, a REPL, a database, …) are drawn by us in a
//! 12-unit grid with a round stroke, and each names the theme colour it takes
//! ([`Tint`]). An agent without a mark of its own shares one generic star.
//!
//! Each mark's source is written above its data. The marks are their owners'
//! trademarks, drawn only to name the program a pane runs; the SVG data of
//! Simple Icons is CC0. To stop drawing one, point its arm in [`agent_icon`]
//! at the generic star — nothing else refers to it.

use crate::program::identity::{Agent, Kind};

mod path;

pub use path::{Fit, PathError, Point, Segment, ViewBox, parse};

/// A kind icon's stroke width in its own units, on the 12-unit grid.
pub const STROKE_WIDTH: f64 = 1.35;

/// How an icon's path is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    /// Filled with the non-zero winding rule, no outline.
    Fill,
    /// An outline `width` wide, with round caps and joins, and no fill. A
    /// zero-length step is a dot.
    Stroke { width: f64 },
}

/// An icon drawn from path data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// SVG path data, in the coordinates of [`Self::view_box`].
    pub d: &'static str,
    pub view_box: ViewBox,
    pub paint: Paint,
}

impl Shape {
    /// The path read into segments, in the view box's units.
    pub fn segments(&self) -> Result<Vec<Segment>, PathError> {
        parse(self.d)
    }
}

/// What an icon is drawn from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Art {
    Path(Shape),
    /// A square PNG whose alpha channel is the shape: the colour channels mean
    /// nothing and the image is drawn tinted, as a template image is.
    Mask(&'static [u8]),
}

/// The colour a kind of icon takes when it is drawn on a tile of its own
/// colour (not the text's): the foreground, one of the theme's status roles or
/// one of the palette's normal ANSI colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Foreground,
    /// The theme's `info` role — also what a remote session's host is drawn in.
    Info,
    /// The theme's `error` role.
    Error,
    Green,
    Magenta,
    Blue,
    Cyan,
    Yellow,
    Red,
}

/// One program's icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Icon {
    /// What the icon shows ("Claude Code", "Shell"), for an accessibility
    /// description.
    pub name: &'static str,
    pub art: Art,
    /// The mark's own colour as `0xRRGGBB`, used only when the user asks for
    /// brand colours. `None` for a mark that is one colour: it stays in the
    /// theme's.
    pub brand: Option<u32>,
    pub tint: Tint,
}

/// The icon of what a pane runs.
pub fn of(kind: Kind) -> &'static Icon {
    match kind {
        Kind::Agent(agent) => agent_icon(agent),
        Kind::Shell => &SHELL,
        Kind::Remote => &REMOTE,
        Kind::Editor => &EDITOR,
        Kind::Repl => &REPL,
        Kind::Database => &DATABASE,
        Kind::Container => &CONTAINER,
        Kind::Monitor => &MONITOR,
        Kind::Git => &GIT,
        Kind::Task => &TASK,
        Kind::Pager => &PAGER,
        Kind::Files => &FILES,
        Kind::Root => &ROOT,
    }
}

/// The icon of a coding agent.
pub fn agent_icon(agent: Agent) -> &'static Icon {
    match agent {
        Agent::ClaudeCode => &CLAUDE_CODE,
        Agent::Codex => &CODEX,
        Agent::OpenCode => &OPENCODE,
        Agent::Antigravity => &ANTIGRAVITY,
        Agent::GeminiCli => &GEMINI_CLI,
        Agent::Pi => &PI,
        Agent::Herdr => &HERDR,
        Agent::CursorCli => &CURSOR_CLI,
        Agent::CopilotCli => &COPILOT_CLI,
        Agent::QwenCode => &QWEN_CODE,
        Agent::KimiCli => &KIMI_CLI,
        Agent::Other => &OTHER_AGENT,
    }
}

impl Icon {
    /// The generic agent star, which an agent without a mark of its own wears.
    pub fn other_agent() -> &'static Self {
        &OTHER_AGENT
    }
}

/// Every kind with an icon, agents included, in the table's order.
pub const KINDS: [Kind; 24] = [
    Kind::Shell,
    Kind::Remote,
    Kind::Agent(Agent::ClaudeCode),
    Kind::Agent(Agent::Codex),
    Kind::Agent(Agent::OpenCode),
    Kind::Agent(Agent::Antigravity),
    Kind::Agent(Agent::GeminiCli),
    Kind::Agent(Agent::Pi),
    Kind::Agent(Agent::Herdr),
    Kind::Agent(Agent::CursorCli),
    Kind::Agent(Agent::CopilotCli),
    Kind::Agent(Agent::QwenCode),
    Kind::Agent(Agent::KimiCli),
    Kind::Agent(Agent::Other),
    Kind::Editor,
    Kind::Repl,
    Kind::Database,
    Kind::Container,
    Kind::Monitor,
    Kind::Git,
    Kind::Task,
    Kind::Pager,
    Kind::Files,
    Kind::Root,
];

/// Simple Icons 16.34.0, slug `claude` (SVG data CC0). The brand colour is the mark's
/// orange.
static CLAUDE_CODE: Icon = Icon {
    name: "Claude Code",
    art: Art::Path(Shape {
        d: "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956\
            -.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797\
            -.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546\
            -.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225\
            -.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 \
            1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539\
            -2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285\
            L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 \
            3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307\
            -2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 \
            1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514\
            -2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 \
            1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 \
            1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614\
            -3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621\
            l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107\
            -1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768\
            -.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17\
            l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721\
            -1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703\
            -.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853\
            -1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 \
            1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389\
            -1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857\
            -.4554.0608-.7467.2307-.2429 1.9064-1.3114Z",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: Some(0xD97757),
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, file `icons/openai.svg`: OpenAI's mark, which is what Codex wears
/// (SVG data CC0).
static CODEX: Icon = Icon {
    name: "Codex",
    art: Art::Path(Shape {
        d: "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098\
            -2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 \
            6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 \
            6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718\
            -4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm\
            -9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582\
            a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052\
            v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1\
            -.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685\
            v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408\
            -1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 \
            .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865\
            A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2\
            a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v\
            -5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818\
            a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615\
            l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638\
            a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l\
            -.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602\
            -1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `opencode` (SVG data CC0).
static OPENCODE: Icon = Icon {
    name: "OpenCode",
    art: Art::Path(Shape {
        d: "M22 24H2V0h20zM17 4.8H7v14.4h10z",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// <https://antigravity.google/favicon.svg>, the arch path only: the original is filled with
/// blurred colours and is flattened to one here. The brand colour is the blue it is filled with.
static ANTIGRAVITY: Icon = Icon {
    name: "Antigravity",
    art: Art::Path(Shape {
        d: "M89.6992 93.695C94.3659 97.195 101.366 94.8617 94.9492 88.445C75.6992 69.7783 \
            79.7825 18.445 55.8659 18.445C31.9492 18.445 36.0325 69.7783 16.7825 88.445\
            C9.78251 95.445 17.3658 97.195 22.0325 93.695C40.1159 81.445 38.9492 59.8617 \
            55.8659 59.8617C72.7825 59.8617 71.6159 81.445 89.6992 93.695Z",
        view_box: ViewBox::new(9.5, 11.0, 92.0, 92.0),
        paint: Paint::Fill,
    }),
    brand: Some(0x3186FF),
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `googlegemini` (SVG data CC0).
static GEMINI_CLI: Icon = Icon {
    name: "Gemini CLI",
    art: Art::Path(Shape {
        d: "M11.04 19.32Q12 21.51 12 24q0-2.49.93-4.68.96-2.19 2.58-3.81t3.81-2.55Q21.51 \
            12 24 12q-2.49 0-4.68-.93a12.3 12.3 0 0 1-3.81-2.58 12.3 12.3 0 0 1-2.58-3.81\
            Q12 2.49 12 0q0 2.49-.96 4.68-.93 2.19-2.55 3.81a12.3 12.3 0 0 1-3.81 2.58\
            Q2.49 12 0 12q2.49 0 4.68.96 2.19.93 3.81 2.55t2.55 3.81",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: Some(0x8E75B2),
    tint: Tint::Foreground,
};

/// <https://pi.dev/favicon.svg>, its three `mark` paths joined.
static PI: Icon = Icon {
    name: "Pi",
    art: Art::Path(Shape {
        d: "M420 280H280V140H0V0H420V280ZM560 560H420V280H560V560ZM140 560H0V140H140V280\
            H280V420H140V560Z",
        view_box: ViewBox::square(560.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// <https://herdr.dev/assets/logo.png>, which is raster only: the pixels darker than a luminance
/// of 140 kept as alpha, cropped to the ram, padded square, 256 px. An alpha mask, drawn tinted.
static HERDR: Icon = Icon {
    name: "Herdr",
    art: Art::Mask(include_bytes!("icon/herdr-mask.png")),
    brand: None,
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `cursor` (SVG data CC0).
static CURSOR_CLI: Icon = Icon {
    name: "Cursor CLI",
    art: Art::Path(Shape {
        d: "M11.503.131 1.891 5.678a.84.84 0 0 0-.42.726v11.188c0 .3.162.575.42.724l9.609 \
            5.55a1 1 0 0 0 .998 0l9.61-5.55a.84.84 0 0 0 .42-.724V6.404a.84.84 0 0 0-.42\
            -.726L12.497.131a1.01 1.01 0 0 0-.996 0M2.657 6.338h18.55c.263 0 \
            .43.287.297.515L12.23 22.918c-.062.107-.229.064-.229-.06V12.335a.59.59 0 0 0\
            -.295-.51l-9.11-5.257c-.109-.063-.064-.23.061-.23",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `githubcopilot` (SVG data CC0).
static COPILOT_CLI: Icon = Icon {
    name: "Copilot CLI",
    art: Art::Path(Shape {
        d: "M23.922 16.997C23.061 18.492 18.063 22.02 12 22.02 5.937 22.02.939 18.492.078 \
            16.997A.641.641 0 0 1 0 16.741v-2.869a.883.883 0 0 1 .053-.22c.372-.935 1.347\
            -2.292 2.605-2.656.167-.429.414-1.055.644-1.517a10.098 10.098 0 0 1-.052-1.086\
            c0-1.331.282-2.499 1.132-3.368.397-.406.89-.717 1.474-.952C7.255 2.937 9.248 \
            1.98 11.978 1.98c2.731 0 4.767.957 6.166 2.093.584.235 1.077.546 \
            1.474.952.85.869 1.132 2.037 1.132 3.368 0 .368-.014.733-.052 \
            1.086.23.462.477 1.088.644 1.517 1.258.364 2.233 1.721 2.605 2.656a.841.841 0 \
            0 1 .053.22v2.869a.641.641 0 0 1-.078.256Zm-11.75-5.992h-.344a4.359 4.359 0 0 \
            1-.355.508c-.77.947-1.918 1.492-3.508 1.492-1.725 0-2.989-.359-3.782-1.259\
            a2.137 2.137 0 0 1-.085-.104L4 11.746v6.585c1.435.779 4.514 2.179 8 2.179 \
            3.486 0 6.565-1.4 8-2.179v-6.585l-.098-.104s-.033.045-.085.104c-.793.9-2.057 \
            1.259-3.782 1.259-1.59 0-2.738-.545-3.508-1.492a4.359 4.359 0 0 1-.355-.508Z\
            m2.328 3.25c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v-2c0\
            -.549.451-1 1-1Zm-5 0c.549 0 1 .451 1 1v2c0 .549-.451 1-1 1-.549 0-1-.451-1-1v\
            -2c0-.549.451-1 1-1Zm3.313-6.185c.136 1.057.403 1.913.878 2.497.442.544 \
            1.134.938 2.344.938 1.573 0 2.292-.337 2.657-.751.384-.435.558-1.15.558-2.361 \
            0-1.14-.243-1.847-.705-2.319-.477-.488-1.319-.862-2.824-1.025-1.487-.161\
            -2.192.138-2.533.529-.269.307-.437.808-.438 1.578v.021c0 .265.021.562.063.893Z\
            m-1.626 0c.042-.331.063-.628.063-.894v-.02c-.001-.77-.169-1.271-.438-1.578\
            -.341-.391-1.046-.69-2.533-.529-1.505.163-2.347.537-2.824 1.025-.462.472-.705 \
            1.179-.705 2.319 0 1.211.175 1.926.558 2.361.365.414 1.084.751 2.657.751 1.21 \
            0 1.902-.394 2.344-.938.475-.584.742-1.44.878-2.497Z",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `qwen` (SVG data CC0).
static QWEN_CODE: Icon = Icon {
    name: "Qwen Code",
    art: Art::Path(Shape {
        d: "M23.919 14.545 20.817 9.17l1.47-2.544a.56.56 0 0 0 0-.566l-1.633-2.83a.57.57 \
            0 0 0-.49-.283h-6.207L12.487.402a.57.57 0 0 0-.49-.284H8.732a.56.56 0 0 0\
            -.49.284L5.139 5.775h-2.94a.56.56 0 0 0-.49.284L.077 8.887a.56.56 0 0 0 0 .567\
            L3.18 14.83l-1.47 2.545a.56.56 0 0 0 0 .566l1.634 2.83a.57.57 0 0 0 .49.283\
            h6.205l1.47 2.545a.57.57 0 0 0 .49.284h3.266a.57.57 0 0 0 .49-.284l3.104-5.375\
            h2.94a.57.57 0 0 0 .49-.283l1.634-2.828a.55.55 0 0 0-.004-.568M8.733.686\
            l1.634 2.828-1.634 2.828H21.8L20.164 9.17H7.425L5.63 6.06Zm1.306 19.801-6.205\
            -.002 1.634-2.83h3.265L2.201 6.344h3.267q3.182 5.517 6.367 11.032zm10.124-5.66\
            L18.53 12l-6.532 11.315-1.634-2.83c2.129-3.673 4.25-7.351 6.373-11.028h3.592\
            l3.102 5.374z",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: Some(0x6950EF),
    tint: Tint::Foreground,
};

/// Simple Icons 16.34.0, slug `kimi` (SVG data CC0).
static KIMI_CLI: Icon = Icon {
    name: "Kimi CLI",
    art: Art::Path(Shape {
        d: "M21.765.351C22.998.351 24 1.353 24 2.586S22.998 4.82 21.765 4.82h-1.974c-.15 0\
            -.26-.12-.26-.26V2.586A2.237 2.237 0 0 1 21.765.35M9.41 13.388l8.447-8.377c.16\
            -.16.07-.471-.14-.471h-4.55s-.1.02-.14.06l-9.099 9.029c-.14.14-.35.02-.35-.21\
            V4.81c0-.15-.1-.27-.221-.27H.22c-.12 0-.22.12-.22.27v18.57c0 .15.1.27.22.27\
            h3.137c.12 0 .22-.12.22-.27v-3.79c0-.08.03-.16.08-.21l2.826-2.796c.07-.07.16\
            -.08.241-.03l7.546 5.551a8.9 8.9 0 0 0 4.018 1.493c.12.01.23-.11.23-.27V19.76\
            c0-.14-.08-.25-.19-.26a5.8 5.8 0 0 1-2.355-.942l-6.533-4.73c-.14-.09-.15-.32\
            -.03-.441",
        view_box: ViewBox::square(24.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// bateri's own drawing: a four-pointed star, for every agent without a mark of its own.
static OTHER_AGENT: Icon = Icon {
    name: "Other agents",
    art: Art::Path(Shape {
        d: "M6 1.2C6.5 4.3 7.7 5.5 10.8 6 7.7 6.5 6.5 7.7 6 10.8 5.5 7.7 4.3 6.5 1.2 6 \
            4.3 5.5 5.5 4.3 6 1.2Z",
        view_box: ViewBox::square(12.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// A prompt: a chevron and a cursor.
static SHELL: Icon = Icon {
    name: "Shell",
    art: Art::Path(Shape {
        d: "M2.5 3.5 5 6 2.5 8.5M6.5 8.75h3.5",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// A globe, in a finer line than the other kinds': its three curves in a square of 12 would close
/// up at theirs. The circle is four curves, a path knows no arcs.
static REMOTE: Icon = Icon {
    name: "Remote session",
    art: Art::Path(Shape {
        d: "M6 1.25C8.62 1.25 10.75 3.38 10.75 6C10.75 8.62 8.62 10.75 6 10.75C3.38 10.75 1.25 8.62 1.25 6C1.25 3.38 3.38 1.25 6 1.25ZM1.25 6h9.5M6 1.25c-2.6 2.9-2.6 6.6 0 9.5M6 1.25c2.6 2.9 2.6 6.6 0 9.5",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke { width: 1.0 },
    }),
    brand: None,
    tint: Tint::Info,
};

/// A pencil.
static EDITOR: Icon = Icon {
    name: "Editor",
    art: Art::Path(Shape {
        d: "M2 10l.6-2.6 5.6-5.6a1 1 0 0 1 1.4 0l.6.6a1 1 0 0 1 0 1.4L4.6 9.4ZM7.4 2.6l2 2",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Green,
};

/// Braces.
static REPL: Icon = Icon {
    name: "Language REPL",
    art: Art::Path(Shape {
        d: "M4.5 2C3 2 3.4 5.4 2 6c1.4.6 1 4 2.5 4M7.5 2C9 2 8.6 5.4 10 6c-1.4.6-1 4-2.5 4",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Magenta,
};

/// A cylinder.
static DATABASE: Icon = Icon {
    name: "Database",
    art: Art::Path(Shape {
        d: "M2.5 3c0-1.3 7-1.3 7 0v6c0 1.3-7 1.3-7 0ZM2.5 3c0 1.3 7 1.3 7 0M2.5 6c0 1.3 7 \
            1.3 7 0",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Blue,
};

/// A cube.
static CONTAINER: Icon = Icon {
    name: "Container",
    art: Art::Path(Shape {
        d: "M6 1.5 10.5 4v4.5L6 11 1.5 8.5V4ZM1.5 4 6 6.5 10.5 4M6 6.5V11",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Cyan,
};

/// A pulse line.
static MONITOR: Icon = Icon {
    name: "System monitor",
    art: Art::Path(Shape {
        d: "M1.5 6.5h2.2l1.4-3 2 6 1.5-3h1.9",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Yellow,
};

/// Two branches.
static GIT: Icon = Icon {
    name: "Git UI",
    art: Art::Path(Shape {
        d: "M4 4v4M4 4a1.4 1.4 0 1 0 0-2.8A1.4 1.4 0 0 0 4 4ZM4 10.8a1.4 1.4 0 1 0 0-2.8 \
            1.4 1.4 0 0 0 0 2.8ZM8.5 5.2a1.4 1.4 0 1 0 0-2.8 1.4 1.4 0 0 0 0 2.8ZM8.5 5.2\
            c0 2-4.5 1.6-4.5 2.8",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Red,
};

/// A bolt, filled.
static TASK: Icon = Icon {
    name: "Task / build",
    art: Art::Path(Shape {
        d: "M7 1.3 2.8 6.9h3.3l-1 3.8L9.3 5H6Z",
        view_box: ViewBox::square(12.0),
        paint: Paint::Fill,
    }),
    brand: None,
    tint: Tint::Cyan,
};

/// A page with lines.
static PAGER: Icon = Icon {
    name: "Document / pager",
    art: Art::Path(Shape {
        d: "M3 1.5h4.2l2 2v7H3ZM4.6 5.2h3.4M4.6 7.2h3.4M4.6 9h2",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Foreground,
};

/// A folder.
static FILES: Icon = Icon {
    name: "File manager",
    art: Art::Path(Shape {
        d: "M1.5 3.2h3.2l1 1.1h4.8v5.2h-9Z",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Blue,
};

/// A hash sign.
static ROOT: Icon = Icon {
    name: "Root shell",
    art: Art::Path(Shape {
        d: "M4.8 2 3.8 10M8.2 2l-1 8M2.2 4.6h8M1.8 7.4h8",
        view_box: ViewBox::square(12.0),
        paint: Paint::Stroke {
            width: STROKE_WIDTH,
        },
    }),
    brand: None,
    tint: Tint::Error,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind's icon name, in [`KINDS`]'s order.
    const NAMES: [&str; 24] = [
        "Shell",
        "Remote session",
        "Claude Code",
        "Codex",
        "OpenCode",
        "Antigravity",
        "Gemini CLI",
        "Pi",
        "Herdr",
        "Cursor CLI",
        "Copilot CLI",
        "Qwen Code",
        "Kimi CLI",
        "Other agents",
        "Editor",
        "Language REPL",
        "Database",
        "Container",
        "System monitor",
        "Git UI",
        "Task / build",
        "Document / pager",
        "File manager",
        "Root shell",
    ];

    fn shapes() -> impl Iterator<Item = (&'static Icon, Shape)> {
        KINDS.iter().filter_map(|&kind| {
            let icon = of(kind);
            match icon.art {
                Art::Path(shape) => Some((icon, shape)),
                Art::Mask(_) => None,
            }
        })
    }

    fn points(segment: &Segment) -> Vec<Point> {
        match *segment {
            Segment::Move(to) | Segment::Line(to) => vec![to],
            Segment::Cubic { c1, c2, to } => vec![c1, c2, to],
            Segment::Close => Vec::new(),
        }
    }

    #[test]
    fn every_kind_has_an_icon_of_its_own() {
        let names: Vec<&str> = KINDS.iter().map(|&kind| of(kind).name).collect();
        assert_eq!(names, NAMES);
        for (i, a) in KINDS.iter().enumerate() {
            for b in &KINDS[i + 1..] {
                assert_ne!(a, b);
                assert!(
                    !std::ptr::eq(of(*a), of(*b)),
                    "{a:?} and {b:?} share an icon"
                );
            }
        }
        assert!(std::ptr::eq(
            of(Kind::Agent(Agent::Other)),
            Icon::other_agent()
        ));
    }

    #[test]
    fn every_path_reads_and_lies_inside_its_view_box() {
        let mut read = 0;
        for (icon, shape) in shapes() {
            let segments = shape
                .segments()
                .unwrap_or_else(|error| panic!("{}: {error}", icon.name));
            assert!(
                matches!(segments.first(), Some(Segment::Move(_))),
                "{} does not start with a move",
                icon.name
            );
            let ViewBox { x, y, w, h } = shape.view_box;
            assert!(w > 0.0 && h > 0.0, "{}: an empty view box", icon.name);
            // The box grown by a little: a curve's control points may stand a
            // hair outside the shape they draw.
            let pad = w.max(h) * 0.02;
            for point in segments.iter().flat_map(points) {
                assert!(
                    point.x.is_finite() && point.y.is_finite(),
                    "{}: {point:?}",
                    icon.name
                );
                assert!(
                    (x - pad..=x + w + pad).contains(&point.x)
                        && (y - pad..=y + h + pad).contains(&point.y),
                    "{}: {point:?} is outside {:?}",
                    icon.name,
                    shape.view_box
                );
            }
            read += 1;
        }
        // Every kind but the one that is a picture.
        assert_eq!(read, KINDS.len() - 1);
    }

    #[test]
    fn a_kind_is_stroked_on_the_twelve_grid_and_a_mark_is_filled() {
        for &kind in &KINDS {
            let icon = of(kind);
            let Art::Path(shape) = icon.art else { continue };
            match (kind, shape.paint) {
                // The bolt is solid; the other kinds are outlines.
                (Kind::Task, Paint::Fill) => {}
                (Kind::Agent(_), Paint::Fill) => {}
                (_, Paint::Stroke { width }) => {
                    assert!(!matches!(kind, Kind::Agent(_)) && kind != Kind::Task);
                    // The globe's three curves would close up at the others' line.
                    let line = if kind == Kind::Remote {
                        1.0
                    } else {
                        STROKE_WIDTH
                    };
                    assert_eq!(width, line, "{}", icon.name);
                    assert_eq!(shape.view_box, ViewBox::square(12.0), "{}", icon.name);
                    // The outline stays inside the box, caps included.
                    let half = width / 2.0;
                    for point in shape.segments().unwrap().iter().flat_map(points) {
                        assert!(
                            (half..=12.0 - half).contains(&point.x)
                                && (half..=12.0 - half).contains(&point.y),
                            "{}: {point:?} clips the stroke",
                            icon.name
                        );
                    }
                }
                (kind, paint) => panic!("{kind:?} is painted {paint:?}"),
            }
        }
        // The generic star is ours, on the grid like the kinds.
        let Art::Path(star) = Icon::other_agent().art else {
            panic!("the star is a path");
        };
        assert_eq!(star.view_box, ViewBox::square(12.0));
    }

    #[test]
    fn four_marks_carry_a_brand_colour_and_the_rest_stay_in_the_theme() {
        let branded: Vec<(&str, u32)> = KINDS
            .iter()
            .filter_map(|&kind| {
                let icon = of(kind);
                icon.brand.map(|brand| (icon.name, brand))
            })
            .collect();
        assert_eq!(
            branded,
            [
                ("Claude Code", 0xD97757),
                ("Antigravity", 0x3186FF),
                ("Gemini CLI", 0x8E75B2),
                ("Qwen Code", 0x6950EF),
            ]
        );
    }

    #[test]
    fn a_kind_takes_its_tint_from_the_theme() {
        let tints: Vec<Tint> = [
            Kind::Shell,
            Kind::Remote,
            Kind::Editor,
            Kind::Repl,
            Kind::Database,
            Kind::Container,
            Kind::Monitor,
            Kind::Git,
            Kind::Task,
            Kind::Pager,
            Kind::Files,
            Kind::Root,
        ]
        .map(|kind| of(kind).tint)
        .to_vec();
        assert_eq!(
            tints,
            [
                Tint::Foreground,
                Tint::Info,
                Tint::Green,
                Tint::Magenta,
                Tint::Blue,
                Tint::Cyan,
                Tint::Yellow,
                Tint::Red,
                Tint::Cyan,
                Tint::Foreground,
                Tint::Blue,
                Tint::Error,
            ]
        );
        // An agent has no colour but the text's, unless its brand supplies one.
        for agent in [Agent::ClaudeCode, Agent::Codex, Agent::Other] {
            assert_eq!(agent_icon(agent).tint, Tint::Foreground);
        }
    }

    /// The CRC-32 of a PNG chunk (IEEE, reflected).
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0_u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn the_herdr_mask_is_a_whole_square_rgba_png() {
        let Art::Mask(png) = of(Kind::Agent(Agent::Herdr)).art else {
            panic!("Herdr's mark is a mask");
        };
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut at = 8;
        let mut chunks = Vec::new();
        while at < png.len() {
            let be = |from: usize| {
                u32::from_be_bytes([png[from], png[from + 1], png[from + 2], png[from + 3]])
            };
            let length = be(at) as usize;
            let kind = &png[at + 4..at + 8];
            let body = &png[at + 8..at + 8 + length];
            assert_eq!(
                crc32(&png[at + 4..at + 8 + length]),
                be(at + 8 + length),
                "chunk {:?} is damaged",
                String::from_utf8_lossy(kind)
            );
            if kind == b"IHDR" {
                assert_eq!(length, 13);
                let dimension = |from: usize| {
                    u32::from_be_bytes([body[from], body[from + 1], body[from + 2], body[from + 3]])
                };
                assert_eq!((dimension(0), dimension(4)), (256, 256));
                // Eight bits per channel, colour type 6 (RGBA), not interlaced.
                assert_eq!((body[8], body[9], body[12]), (8, 6, 0));
            }
            chunks.push(String::from_utf8_lossy(kind).into_owned());
            at += 12 + length;
        }
        assert_eq!(at, png.len(), "bytes after the last chunk");
        assert_eq!(chunks.first().map(String::as_str), Some("IHDR"));
        assert_eq!(chunks.last().map(String::as_str), Some("IEND"));
        assert!(chunks.iter().any(|chunk| chunk == "IDAT"));
    }

    #[test]
    fn a_mark_lands_inside_the_slot_it_is_fitted_to() {
        for (icon, shape) in shapes() {
            let fit = shape.view_box.fit(12.0);
            let pad = 12.0 * 0.02;
            for point in shape
                .segments()
                .unwrap()
                .into_iter()
                .map(|segment| fit.segment(segment))
                .flat_map(|segment| points(&segment))
            {
                assert!(
                    (-pad..=12.0 + pad).contains(&point.x)
                        && (-pad..=12.0 + pad).contains(&point.y),
                    "{}: {point:?} is outside a 12-point slot",
                    icon.name
                );
            }
        }
    }
}

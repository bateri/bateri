//! Pencerenin içeriği: `CAMetalLayer`'ı taşıyan ve klavyeyi PTY'ye akıtan view.
//!
//! Çizim burada **yok** — layer'ın içeriğini `bt-gpu` doldurur. Bu sınıfın tek
//! işi first responder olmak ve tuş vuruşunu [`crate::keys::encode_key`]'e
//! verip çıkan baytları oturuma yazmak.

use std::cell::OnceCell;
use std::sync::Arc;

use bt_core::Session;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSView};
use objc2_foundation::{NSObjectProtocol, NSRect};

use crate::keys::encode_key;

pub(crate) struct ViewIvars {
    /// View, oturumdan **önce** doğmak zorunda: grid ölçüsü contentView'ın
    /// bounds'undan türüyor ve `Session::spawn` o ölçüyü istiyor. Bir tuş
    /// vuruşu arada geçemez ama sebebi pencerenin henüz key olmaması değil
    /// (`makeKeyAndOrderFront` daha önce koşuyor): boşluk
    /// `applicationDidFinishLaunching`'in içinde, **run loop dönmeden**
    /// kapanıyor, yani araya hiçbir olay düşemiyor.
    session: OnceCell<Arc<Session>>,
}

define_class!(
    // SAFETY: NSView alt sınıflama için tasarlanmıştır; BateriView `Drop`
    // uygulamaz ve `initWithFrame:` dışında bir kurucu sunmaz.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriView"]
    #[ivars = ViewIvars]
    pub(crate) struct BateriView;

    unsafe impl NSObjectProtocol for BateriView {}

    impl BateriView {
        /// Tuş vuruşlarının buraya gelmesinin şartı. `NSView`'un varsayılanı
        /// `false`; `makeFirstResponder` bu olmadan sessizce reddedilir.
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // `characters` modifier'lar uygulanmış hâli verir (Option-basılı
            // "ø", Ctrl-C → U+0003); ham tuş kodu `charactersIgnoringModifiers`
            // olurdu ve klavye düzenini bizim yeniden uygulamamızı isterdi.
            let Some(chars) = event.characters() else {
                return;
            };
            let flags = event.modifierFlags();
            // Command basılıyken tuş bir kısayoldur, girdi değil. Ana menü
            // henüz yok (00X) ve menüsüz bir uygulamada `performKeyEquivalent:`
            // hiçbir şeyi yakalamıyor: bu satır olmadan Cmd-V shell'e "v",
            // Cmd-W "w" yazardı. Yutuyoruz — menü gelene kadar doğru davranış
            // hiçbir şey yapmamak.
            if flags.contains(NSEventModifierFlags::Command) {
                return;
            }
            let ctrl = flags.contains(NSEventModifierFlags::Control);
            let Some(session) = self.ivars().session.get() else {
                return;
            };
            // `super`'e geçmiyoruz: `NSResponder::keyDown:` tanımadığı tuşta
            // beep çalar ve terminalde her ok tuşu bip sesi olurdu.
            if let Some(bytes) = encode_key(&chars.to_string(), ctrl) {
                session.write(&bytes);
            }
        }
    }
);

impl BateriView {
    pub(crate) fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars {
            session: OnceCell::new(),
        });
        // SAFETY: `initWithFrame:` NSView'un tasarlanmış kurucusu ve ivar'lar
        // set edildi.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Oturumu bağlar; bu andan sonra tuşlar PTY'ye gider.
    pub(crate) fn attach(&self, session: Arc<Session>) {
        // İkinci çağrı sessizce düşseydi tuşlar eski oturuma giderdi ve
        // pencere yazmıyor gibi görünürdü — tek satır iz bile bırakmadan.
        assert!(
            self.ivars().session.set(session).is_ok(),
            "oturum ikinci kez bağlandı"
        );
    }
}
